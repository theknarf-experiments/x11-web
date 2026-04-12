/*
 * dbusmenu-test: publishes a static dbusmenu tree on the session bus,
 * registers it with com.canonical.AppMenu.Registrar, maps an X11
 * window, and waits for SIGTERM. Used by e2e tests to verify that our
 * sidecar's menu tracker pipeline works end-to-end.
 *
 * Build: gcc -o dbusmenu-test dbusmenu-test.c \
 *            $(pkg-config --cflags --libs dbusmenu-glib-0.4 x11)
 */

#include <libdbusmenu-glib/menuitem.h>
#include <libdbusmenu-glib/server.h>
#include <X11/Xlib.h>
#include <glib.h>
#include <gio/gio.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static GMainLoop *loop = NULL;

static void on_signal(int sig) {
    (void)sig;
    if (loop) g_main_loop_quit(loop);
}

/* Register the window's dbusmenu path with the Registrar. */
static void register_window(GDBusConnection *bus, guint32 xid, const char *path) {
    GError *err = NULL;
    g_dbus_connection_call_sync(
        bus,
        "com.canonical.AppMenu.Registrar",
        "/com/canonical/AppMenu/Registrar",
        "com.canonical.AppMenu.Registrar",
        "RegisterWindow",
        g_variant_new("(uo)", xid, path),
        NULL, G_DBUS_CALL_FLAGS_NONE, 3000, NULL, &err);
    if (err) {
        fprintf(stderr, "RegisterWindow: %s\n", err->message);
        g_error_free(err);
    } else {
        printf("Registered window 0x%x at %s\n", xid, path);
    }
}

int main(int argc, char **argv) {
    (void)argc; (void)argv;

    signal(SIGTERM, on_signal);
    signal(SIGINT, on_signal);

    /* Open X display and create a mapped window. */
    Display *dpy = XOpenDisplay(NULL);
    if (!dpy) { fprintf(stderr, "Cannot open display\n"); return 1; }

    int screen = DefaultScreen(dpy);
    Window win = XCreateSimpleWindow(dpy, RootWindow(dpy, screen),
        10, 10, 400, 300, 1,
        BlackPixel(dpy, screen), WhitePixel(dpy, screen));

    XStoreName(dpy, win, "dbusmenu-test");
    XMapWindow(dpy, win);
    XFlush(dpy);

    /* Build a simple menu tree. */
    loop = g_main_loop_new(NULL, FALSE);

    DbusmenuServer *server = dbusmenu_server_new("/MenuBar");
    DbusmenuMenuitem *root = dbusmenu_menuitem_new();

    /* File submenu */
    DbusmenuMenuitem *file_item = dbusmenu_menuitem_new();
    dbusmenu_menuitem_property_set(file_item, DBUSMENU_MENUITEM_PROP_LABEL, "File");
    dbusmenu_menuitem_property_set(file_item, DBUSMENU_MENUITEM_PROP_CHILD_DISPLAY,
        DBUSMENU_MENUITEM_CHILD_DISPLAY_SUBMENU);

    DbusmenuMenuitem *new_item = dbusmenu_menuitem_new();
    dbusmenu_menuitem_property_set(new_item, DBUSMENU_MENUITEM_PROP_LABEL, "New");
    dbusmenu_menuitem_property_set(new_item, DBUSMENU_MENUITEM_PROP_SHORTCUT, "Ctrl+N");

    DbusmenuMenuitem *open_item = dbusmenu_menuitem_new();
    dbusmenu_menuitem_property_set(open_item, DBUSMENU_MENUITEM_PROP_LABEL, "Open");

    DbusmenuMenuitem *quit_item = dbusmenu_menuitem_new();
    dbusmenu_menuitem_property_set(quit_item, DBUSMENU_MENUITEM_PROP_LABEL, "Quit");

    dbusmenu_menuitem_child_append(file_item, new_item);
    dbusmenu_menuitem_child_append(file_item, open_item);
    dbusmenu_menuitem_child_append(file_item, quit_item);
    dbusmenu_menuitem_child_append(root, file_item);

    /* Edit submenu */
    DbusmenuMenuitem *edit_item = dbusmenu_menuitem_new();
    dbusmenu_menuitem_property_set(edit_item, DBUSMENU_MENUITEM_PROP_LABEL, "Edit");
    dbusmenu_menuitem_property_set(edit_item, DBUSMENU_MENUITEM_PROP_CHILD_DISPLAY,
        DBUSMENU_MENUITEM_CHILD_DISPLAY_SUBMENU);

    DbusmenuMenuitem *cut_item = dbusmenu_menuitem_new();
    dbusmenu_menuitem_property_set(cut_item, DBUSMENU_MENUITEM_PROP_LABEL, "Cut");

    DbusmenuMenuitem *copy_item = dbusmenu_menuitem_new();
    dbusmenu_menuitem_property_set(copy_item, DBUSMENU_MENUITEM_PROP_LABEL, "Copy");

    DbusmenuMenuitem *paste_item = dbusmenu_menuitem_new();
    dbusmenu_menuitem_property_set(paste_item, DBUSMENU_MENUITEM_PROP_LABEL, "Paste");

    dbusmenu_menuitem_child_append(edit_item, cut_item);
    dbusmenu_menuitem_child_append(edit_item, copy_item);
    dbusmenu_menuitem_child_append(edit_item, paste_item);
    dbusmenu_menuitem_child_append(root, edit_item);

    /* Help submenu */
    DbusmenuMenuitem *help_item = dbusmenu_menuitem_new();
    dbusmenu_menuitem_property_set(help_item, DBUSMENU_MENUITEM_PROP_LABEL, "Help");
    dbusmenu_menuitem_property_set(help_item, DBUSMENU_MENUITEM_PROP_CHILD_DISPLAY,
        DBUSMENU_MENUITEM_CHILD_DISPLAY_SUBMENU);

    DbusmenuMenuitem *about_item = dbusmenu_menuitem_new();
    dbusmenu_menuitem_property_set(about_item, DBUSMENU_MENUITEM_PROP_LABEL, "About");
    dbusmenu_menuitem_child_append(help_item, about_item);
    dbusmenu_menuitem_child_append(root, help_item);

    dbusmenu_server_set_root(server, root);

    /* Register with the AppMenu registrar. */
    GError *err = NULL;
    GDBusConnection *bus = g_bus_get_sync(G_BUS_TYPE_SESSION, NULL, &err);
    if (bus) {
        register_window(bus, (guint32)win, "/MenuBar");
    } else {
        fprintf(stderr, "Session bus: %s\n", err ? err->message : "unknown");
        if (err) g_error_free(err);
    }

    printf("dbusmenu-test ready (window 0x%lx)\n", win);
    fflush(stdout);

    g_main_loop_run(loop);

    /* Cleanup */
    g_object_unref(server);
    g_main_loop_unref(loop);
    XDestroyWindow(dpy, win);
    XCloseDisplay(dpy);
    return 0;
}
