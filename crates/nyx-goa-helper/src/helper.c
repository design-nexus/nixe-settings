/* libgoa-backend add/list/refresh. Runs in its own process so Nyx Settings
 * never links libadwaita. Does not print tokens. */

#include <goa/goa.h>
#include <goabackend/goabackend.h>
#include <gtk/gtk.h>
#include <stdio.h>
#include <string.h>

static void
json_str(FILE *out, const char *s)
{
    fputc('"', out);
    for (; s && *s; s++) {
        if (*s == '\\' || *s == '"') {
            fputc('\\', out);
            fputc(*s, out);
        } else if (*s == '\n') {
            fputs("\\n", out);
        } else {
            fputc(*s, out);
        }
    }
    fputc('"', out);
}

typedef struct {
    GMainLoop *loop;
    GList *providers;
    GError *error;
} ListData;

static void
on_providers(GObject *src, GAsyncResult *res, gpointer user_data)
{
    ListData *d = user_data;
    (void)src;
    goa_provider_get_all_finish(&d->providers, res, &d->error);
    g_main_loop_quit(d->loop);
}

static int
cmd_list(void)
{
    ListData d = { 0 };
    gboolean first = TRUE;

    gtk_init();
    d.loop = g_main_loop_new(NULL, FALSE);
    goa_provider_get_all(on_providers, &d);
    g_main_loop_run(d.loop);
    if (d.error != NULL) {
        fprintf(stderr, "%s\n", d.error->message);
        return 1;
    }

    fputc('[', stdout);
    for (int pass = 0; pass < 2; pass++) {
        for (GList *l = d.providers; l != NULL; l = l->next) {
            GoaProvider *p = l->data;
            gboolean branded = (goa_provider_get_provider_features(p) & GOA_PROVIDER_FEATURE_BRANDED) != 0;
            const char *type;
            char *name;

            if ((pass == 0) != branded) {
                continue;
            }
            type = goa_provider_get_provider_type(p);
            name = goa_provider_get_provider_name(p, NULL);
            if (!first) {
                fputc(',', stdout);
            }
            first = FALSE;
            fputs("{\"type\":", stdout);
            json_str(stdout, type);
            fputs(",\"name\":", stdout);
            json_str(stdout, name != NULL ? name : type);
            fputc('}', stdout);
            g_free(name);
        }
    }
    fputs("]\n", stdout);
    g_list_free_full(d.providers, g_object_unref);
    g_main_loop_unref(d.loop);
    return 0;
}

typedef struct {
    GMainLoop *loop;
    GError *error;
    gboolean ok;
} TaskData;

static void
on_add(GObject *src, GAsyncResult *res, gpointer user_data)
{
    TaskData *d = user_data;
    GoaObject *obj = goa_provider_add_account_finish(GOA_PROVIDER(src), res, &d->error);
    d->ok = obj != NULL;
    g_clear_object(&obj);
    g_main_loop_quit(d->loop);
}

static void
on_refresh(GObject *src, GAsyncResult *res, gpointer user_data)
{
    TaskData *d = user_data;
    d->ok = goa_provider_refresh_account_finish(GOA_PROVIDER(src), res, &d->error);
    g_main_loop_quit(d->loop);
}

static GoaObject *
find_account(GoaClient *client, const char *id)
{
    GList *list = goa_client_get_accounts(client);
    GoaObject *found = NULL;

    for (GList *l = list; l != NULL; l = l->next) {
        GoaObject *obj = l->data;
        GoaAccount *acc = goa_object_peek_account(obj);
        const char *aid;
        const char *path;

        if (acc == NULL) {
            continue;
        }
        aid = goa_account_get_id(acc);
        path = g_dbus_object_get_object_path(G_DBUS_OBJECT(obj));
        if (g_strcmp0(aid, id) == 0 || g_strcmp0(path, id) == 0) {
            found = g_object_ref(obj);
            break;
        }
    }
    g_list_free_full(list, g_object_unref);
    return found;
}

static int
finish_task(TaskData *d)
{
    int rc = 0;
    if (d->error != NULL) {
        fprintf(stderr, "%s\n", d->error->message);
        rc = g_error_matches(d->error, G_IO_ERROR, G_IO_ERROR_CANCELLED) ? 2 : 1;
        g_error_free(d->error);
    } else if (!d->ok) {
        rc = 1;
    }
    g_main_loop_unref(d->loop);
    return rc;
}

static int
cmd_add(const char *type)
{
    GError *err = NULL;
    GoaProvider *provider;
    GoaClient *client;
    GtkWidget *parent;
    TaskData d = { 0 };

    gtk_init();
    provider = goa_provider_get_for_provider_type(type);
    if (provider == NULL) {
        fprintf(stderr, "Unknown provider\n");
        return 1;
    }
    client = goa_client_new_sync(NULL, &err);
    if (client == NULL) {
        fprintf(stderr, "%s\n", err->message);
        g_error_free(err);
        return 1;
    }
    parent = GTK_WIDGET(gtk_window_new());
    gtk_window_set_title(GTK_WINDOW(parent), "Online Accounts");
    d.loop = g_main_loop_new(NULL, FALSE);
    goa_provider_add_account(provider, client, parent, NULL, on_add, &d);
    g_main_loop_run(d.loop);
    gtk_window_destroy(GTK_WINDOW(parent));
    g_object_unref(client);
    g_object_unref(provider);
    return finish_task(&d);
}

static int
cmd_refresh(const char *id)
{
    GError *err = NULL;
    GoaClient *client;
    GoaObject *object;
    GoaAccount *acc;
    GoaProvider *provider;
    GtkWidget *parent;
    TaskData d = { 0 };
    const char *type;

    gtk_init();
    client = goa_client_new_sync(NULL, &err);
    if (client == NULL) {
        fprintf(stderr, "%s\n", err->message);
        g_error_free(err);
        return 1;
    }
    object = find_account(client, id);
    if (object == NULL) {
        fprintf(stderr, "That account isn't here.\n");
        g_object_unref(client);
        return 1;
    }
    acc = goa_object_peek_account(object);
    type = goa_account_get_provider_type(acc);
    provider = goa_provider_get_for_provider_type(type);
    if (provider == NULL) {
        fprintf(stderr, "Unknown provider\n");
        g_object_unref(object);
        g_object_unref(client);
        return 1;
    }
    parent = GTK_WIDGET(gtk_window_new());
    gtk_window_set_title(GTK_WINDOW(parent), "Online Accounts");
    d.loop = g_main_loop_new(NULL, FALSE);
    goa_provider_refresh_account(provider, client, object, parent, NULL, on_refresh, &d);
    g_main_loop_run(d.loop);
    gtk_window_destroy(GTK_WINDOW(parent));
    g_object_unref(provider);
    g_object_unref(object);
    g_object_unref(client);
    return finish_task(&d);
}

int
nyx_goa_run(int argc, char **argv)
{
    if (argc >= 2 && strcmp(argv[1], "list") == 0) {
        return cmd_list();
    }
    if (argc >= 3 && strcmp(argv[1], "add") == 0) {
        return cmd_add(argv[2]);
    }
    if (argc >= 3 && strcmp(argv[1], "refresh") == 0) {
        return cmd_refresh(argv[2]);
    }
    fprintf(stderr, "usage: nyx-goa-helper list | add TYPE | refresh ID\n");
    return 1;
}
