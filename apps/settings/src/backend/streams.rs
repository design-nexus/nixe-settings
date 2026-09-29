//! Apps playing sound right now (PulseAudio "sink inputs" on PipeWire): their
//! volume, mute and which output they go to.

use crate::cmd;
use anyhow::Result;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    pub index: u32,
    pub app: String,
    /// What it's playing, e.g. a tab title, when the app says.
    pub media: String,
    pub icon: String,
    /// Name of the sink it plays to.
    pub sink: String,
    pub volume: u32,
    pub muted: bool,
}

/// Parse `pactl -f json list sink-inputs`, given `pactl -f json list sinks` to
/// turn sink numbers into names. Settings' own equalizer stream is left out.
pub fn parse(inputs: &str, sinks: &str) -> Vec<Stream> {
    let sinks: Vec<Value> = serde_json::from_str(sinks).unwrap_or_default();
    let sink_name = |i: u64| {
        sinks
            .iter()
            .find(|s| s.get("index").and_then(|x| x.as_u64()) == Some(i))
            .and_then(|s| s.get("name").and_then(|n| n.as_str()))
            .unwrap_or("")
            .to_string()
    };
    let inputs: Vec<Value> = serde_json::from_str(inputs).unwrap_or_default();
    inputs
        .iter()
        .filter_map(|v| {
            let props = v.get("properties")?;
            let prop = |k: &str| props.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
            // Filter chains and loopbacks (equalizers, speaker tuning) aren't apps
            // and say so by not naming one.
            let app = [prop("application.name"), prop("application.process.binary")].into_iter().find(|s| !s.is_empty())?;
            if prop("node.name").starts_with(super::audio::SINK) {
                return None;
            }
            let media = prop("media.name");
            // Average of the channels, as a whole percent.
            let chans: Vec<u32> = v
                .get("volume")
                .and_then(|x| x.as_object())
                .map(|o| {
                    o.values()
                        .filter_map(|c| c.get("value_percent").and_then(|p| p.as_str()))
                        .filter_map(|p| p.trim().trim_end_matches('%').parse().ok())
                        .collect()
                })
                .unwrap_or_default();
            let volume = if chans.is_empty() { 100 } else { chans.iter().sum::<u32>() / chans.len() as u32 };
            Some(Stream {
                index: v.get("index")?.as_u64()? as u32,
                media: if media == app { String::new() } else { media },
                app,
                icon: prop("application.icon_name"),
                sink: v.get("sink").and_then(|x| x.as_u64()).map(sink_name).unwrap_or_default(),
                volume,
                muted: v.get("mute").and_then(|x| x.as_bool()).unwrap_or(false),
            })
        })
        .collect()
}

pub fn list() -> Vec<Stream> {
    let inputs = cmd::output(&["pactl", "-f", "json", "list", "sink-inputs"]).unwrap_or_default();
    let sinks = cmd::output(&["pactl", "-f", "json", "list", "sinks"]).unwrap_or_default();
    parse(&inputs, &sinks)
}

pub fn set_volume(index: u32, percent: u32) {
    cmd::spawn(&["pactl", "set-sink-input-volume", &index.to_string(), &format!("{percent}%")]);
}

pub fn set_mute(index: u32, muted: bool) {
    cmd::spawn(&["pactl", "set-sink-input-mute", &index.to_string(), if muted { "1" } else { "0" }]);
}

pub fn move_to(index: u32, sink: &str) -> Result<String> {
    cmd::run(&["pactl", "move-sink-input", &index.to_string(), sink])
}

#[cfg(test)]
mod tests {
    use super::*;

    const SINKS: &str = r#"[{"index":55,"name":"alsa_output.pci.analog-stereo","description":"Speakers"},
        {"index":70,"name":"settings_eq","description":"Equalizer"}]"#;
    const INPUTS: &str = r#"[
      {"index":101,"sink":55,"mute":false,
       "volume":{"front-left":{"value":42598,"value_percent":"65%","db":"-11dB"},"front-right":{"value":42598,"value_percent":"65%","db":"-11dB"}},
       "properties":{"application.name":"Firefox","media.name":"Lo-fi beats - YouTube","application.icon_name":"firefox","node.name":"Firefox"}},
      {"index":102,"sink":55,"mute":true,
       "volume":{"mono":{"value":65536,"value_percent":"100%","db":"0dB"}},
       "properties":{"application.process.binary":"mpv","media.name":"mpv"}},
      {"index":103,"sink":55,"mute":false,"volume":{},
       "properties":{"node.name":"settings_eq.output","application.name":"pipewire"}},
      {"index":104,"sink":55,"mute":false,"volume":{},
       "properties":{"node.name":"speaker_tuning.output","media.name":"Speaker tuning","media.class":"Stream/Output/Audio"}}
    ]"#;

    #[test]
    fn parses_app_streams() {
        let s = parse(INPUTS, SINKS);
        assert_eq!(s.len(), 2, "the equalizer and other filter streams are hidden");
        assert_eq!(s[0].app, "Firefox");
        assert_eq!(s[0].media, "Lo-fi beats - YouTube");
        assert_eq!(s[0].volume, 65);
        assert_eq!(s[0].sink, "alsa_output.pci.analog-stereo");
        assert!(!s[0].muted);
        assert_eq!(s[1].app, "mpv");
        assert_eq!(s[1].media, "", "media name that repeats the app is dropped");
        assert!(s[1].muted);
        assert!(parse("", "").is_empty());
    }
}
