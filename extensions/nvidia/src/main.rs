//! NVIDIA graphics through nvidia-smi: what the GPU is doing (temperature,
//! use, memory, clocks, power), which programs use it, the power limit and
//! persistence mode. Fans are shown only: on Wayland the driver offers no
//! way to set them.

use anyhow::{Context, Result, bail};
use nyx_ext::page::{self, Page, desc};
use nyx_ext::{Extension, sys};
use serde_json::{Value, json};

const FIELDS: &str = "index,name,driver_version,temperature.gpu,utilization.gpu,memory.used,memory.total,power.draw,power.limit,power.min_limit,power.max_limit,clocks.gr,clocks.mem,fan.speed,persistence_mode,pstate,pci.bus_id";

#[derive(Debug, Clone, Default, PartialEq)]
struct Gpu {
    index: u32,
    name: String,
    driver: String,
    temp: Option<f64>,
    util: Option<f64>,
    mem_used: Option<f64>,
    mem_total: Option<f64>,
    power: Option<f64>,
    limit: Option<f64>,
    limit_min: Option<f64>,
    limit_max: Option<f64>,
    clock: Option<f64>,
    mem_clock: Option<f64>,
    fan: Option<f64>,
    persistence: bool,
    pstate: String,
    bus: String,
}

/// A number from nvidia-smi's csv ("[N/A]", "[Not Supported]" are none).
fn num(s: &str) -> Option<f64> {
    s.trim().split_whitespace().next()?.parse().ok()
}

fn parse(text: &str) -> Vec<Gpu> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').map(str::trim).collect();
            if f.len() < 17 {
                return None;
            }
            Some(Gpu {
                index: f[0].parse().ok()?,
                name: f[1].to_string(),
                driver: f[2].to_string(),
                temp: num(f[3]),
                util: num(f[4]),
                mem_used: num(f[5]),
                mem_total: num(f[6]),
                power: num(f[7]),
                limit: num(f[8]),
                limit_min: num(f[9]),
                limit_max: num(f[10]),
                clock: num(f[11]),
                mem_clock: num(f[12]),
                fan: num(f[13]),
                persistence: f[14] == "Enabled",
                pstate: f[15].to_string(),
                bus: f[16].to_string(),
            })
        })
        .collect()
}

/// (pid, program, MiB) for programs using a GPU.
fn parse_apps(text: &str) -> Vec<(String, String, String)> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').map(str::trim).collect();
            (f.len() >= 3).then(|| (f[0].to_string(), f[1].rsplit('/').next().unwrap_or(f[1]).to_string(), f[2].to_string()))
        })
        .collect()
}

enum State {
    Ok(Vec<Gpu>),
    /// The driver was updated; the computer needs a restart to load it.
    Mismatch,
    /// nvidia-smi failed some other way.
    Failed(String),
}

fn query() -> State {
    match sys::run("nvidia-smi", &[&format!("--query-gpu={FIELDS}"), "--format=csv,noheader,nounits"]) {
        Ok(t) => State::Ok(parse(&t)),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("version mismatch") { State::Mismatch } else { State::Failed(msg) }
        }
    }
}

fn one(v: Option<f64>, unit: &str) -> String {
    v.map(|x| format!("{}{unit}", if x.fract() == 0.0 { format!("{x:.0}") } else { format!("{x:.1}") })).unwrap_or_else(|| "—".into())
}

struct Nvidia;

impl Extension for Nvidia {
    fn pages(&self) -> Value {
        if !sys::present("nvidia-smi") {
            return json!([]);
        }
        match query() {
            State::Ok(gpus) => Value::Array(
                gpus.iter()
                    .map(|g| {
                        page::entry(&format!("gpu{}", g.index), &g.name.trim_start_matches("NVIDIA ").to_string(), "video-display-symbolic",
                            "Temperature, use, memory, power limit and which programs use the GPU.",
                            "nvidia gpu graphics card temperature power limit vram memory cuda persistence")
                    })
                    .collect(),
            ),
            // Still a page, to say what's wrong.
            _ => json!([page::entry("gpu0", "NVIDIA", "video-display-symbolic", "NVIDIA graphics.", "nvidia gpu graphics")]),
        }
    }

    fn describe(&self, id: &str) -> Result<Value> {
        let gpus = match query() {
            State::Ok(g) => g,
            State::Mismatch => {
                return Ok(Page::new("")
                    .banner("The NVIDIA driver was updated. Restart the computer to finish: until then the GPU can't be read, and games and CUDA may not start.", true)
                    .build());
            }
            State::Failed(e) => return Ok(Page::new("").banner(&format!("nvidia-smi couldn't read the GPU: {}", e.replace('<', "&lt;")), true).build()),
        };
        let g = gpus.into_iter().find(|g| format!("gpu{}", g.index) == id).context("that GPU isn't there any more")?;
        let mut now = vec![
            page::info("temp", "Temperature", &one(g.temp, " °C")),
            page::info("util", "In use", &one(g.util, " %")),
            page::info("mem", "Memory", &format!("{} of {}", one(g.mem_used.map(|m| m / 1024.0), " GB"), one(g.mem_total.map(|m| m / 1024.0), " GB"))),
            page::info("power", "Power draw", &one(g.power, " W")),
            page::info("clock", "Clocks", &format!("{} graphics · {} memory", one(g.clock, " MHz"), one(g.mem_clock, " MHz"))),
        ];
        if g.fan.is_some() {
            now.push(desc(page::info("fan", "Fan", &one(g.fan, " %")), "Set by the driver; it can't be changed on Wayland."));
        }
        now.push(page::info("pstate", "Performance state", &format!("{} (P0 is fastest, P8 idle)", g.pstate)));
        let mut control = Vec::new();
        if let (Some(lim), Some(lo), Some(hi)) = (g.limit, g.limit_min, g.limit_max)
            && hi > lo
        {
            control.push(desc(
                page::slider("limit", "Power limit", lim, (lo.ceil(), hi.floor(), 5.0), " W"),
                "Lower runs cooler and quieter, a little slower. Asks for your password; back to the default at restart.",
            ));
        }
        control.push(desc(
            page::switch("persistence", "Keep the driver loaded", g.persistence),
            "Programs start using the GPU faster, at the cost of a little power while it's idle. Asks for your password.",
        ));
        let apps = parse_apps(&sys::run("nvidia-smi", &["--query-compute-apps=pid,process_name,used_memory", "--format=csv,noheader,nounits"]).unwrap_or_default());
        let app_rows: Vec<Value> = if apps.is_empty() {
            vec![page::info("none", "Programs", "None right now")]
        } else {
            apps.iter().map(|(pid, name, mem)| page::info(&format!("app{pid}"), name, &format!("{mem} MB"))).collect()
        };
        let info = vec![page::info("driver", "Driver", &g.driver), page::info("bus", "PCI address", &g.bus)];
        Ok(Page::new(&format!("<b>{}</b> · driver {}", g.name, g.driver))
            .poll(3)
            .group("Now", "", now)
            .group("Programs using it", "", app_rows)
            .group("Control", "", control)
            .group("Card", "", info)
            .build())
    }

    fn set(&self, page: &str, key: &str, value: &str) -> Result<Option<Value>> {
        let index = page.trim_start_matches("gpu");
        if index.parse::<u32>().is_err() {
            bail!("no such GPU");
        }
        match key {
            "limit" => {
                let w = value.parse::<f64>()?.round() as u32;
                sys::run_root("nvidia-smi", &["-i", index, "-pl", &w.to_string()])?;
                Ok(nyx_ext::toast(&format!("Power limit {w} W")))
            }
            "persistence" => {
                sys::run_root("nvidia-smi", &["-i", index, "-pm", if value == "true" { "1" } else { "0" }])?;
                Ok(None)
            }
            _ => bail!("unknown setting {key}"),
        }
    }
}

fn main() {
    nyx_ext::run(&Nvidia);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_gpus() {
        let t = "0, NVIDIA GeForce RTX 5080 Laptop GPU, 615.78, 45, 12, 1024, 16303, 18.52, 150.00, 80.00, 175.00, 1200, 7000, [N/A], Disabled, P8, 00000000:01:00.0\n";
        let g = &parse(t)[0];
        assert_eq!(g.name, "NVIDIA GeForce RTX 5080 Laptop GPU");
        assert_eq!(g.temp, Some(45.0));
        assert_eq!(g.limit_max, Some(175.0));
        assert_eq!(g.fan, None);
        assert!(!g.persistence);
        assert_eq!(g.pstate, "P8");
        assert!(parse("garbage\n").is_empty());
    }

    #[test]
    fn reads_programs() {
        let a = parse_apps("1234, /usr/bin/ollama, 5300\n");
        assert_eq!(a, vec![("1234".into(), "ollama".into(), "5300".into())]);
        assert_eq!(one(Some(16.0), " GB"), "16 GB");
        assert_eq!(one(None, " W"), "—");
    }
}
