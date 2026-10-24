// vpclick X Y [click|move|scroll N]: a pointer for a compositor with none.
//
// A headless sway seat has no pointer device, so `swaymsg seat cursor` moves
// nothing a client sees. This creates a wlr virtual pointer, moves it to
// X Y (pixels on the 1600x1000 output session.sh makes), and clicks, only
// moves, or scrolls N notches down (negative: up). Built by session.sh.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wayland-client.h>
#include "virtual-pointer.h"
static struct zwlr_virtual_pointer_manager_v1 *mgr;
static void reg(void *d, struct wl_registry *r, uint32_t name, const char *iface, uint32_t v) {
  if (!strcmp(iface, zwlr_virtual_pointer_manager_v1_interface.name))
    mgr = wl_registry_bind(r, name, &zwlr_virtual_pointer_manager_v1_interface, 1);
}
static void unreg(void *d, struct wl_registry *r, uint32_t n) {}
static const struct wl_registry_listener rl = { reg, unreg };
int main(int argc, char **argv) {
  if (argc < 3) return 2;
  int x = atoi(argv[1]), y = atoi(argv[2]);
  const char *what = argc > 3 ? argv[3] : "click";
  struct wl_display *d = wl_display_connect(NULL);
  if (!d) { fprintf(stderr, "no display\n"); return 1; }
  struct wl_registry *r = wl_display_get_registry(d);
  wl_registry_add_listener(r, &rl, NULL);
  wl_display_roundtrip(d);
  if (!mgr) { fprintf(stderr, "no virtual pointer manager\n"); return 1; }
  struct zwlr_virtual_pointer_v1 *p = zwlr_virtual_pointer_manager_v1_create_virtual_pointer(mgr, NULL);
  zwlr_virtual_pointer_v1_motion_absolute(p, 0, x, y, 1600, 1000);
  zwlr_virtual_pointer_v1_frame(p);
  wl_display_roundtrip(d); usleep(80000);
  if (!strcmp(what, "click")) {
    zwlr_virtual_pointer_v1_button(p, 0, 272, 1); zwlr_virtual_pointer_v1_frame(p);
    wl_display_roundtrip(d); usleep(60000);
    zwlr_virtual_pointer_v1_button(p, 0, 272, 0); zwlr_virtual_pointer_v1_frame(p);
  } else if (!strcmp(what, "scroll")) {
    int n = argc > 4 ? atoi(argv[4]) : 3;
    zwlr_virtual_pointer_v1_axis_source(p, 1); /* finger: a wheel with no discrete step scrolls nothing */
    zwlr_virtual_pointer_v1_axis(p, 0, 0, wl_fixed_from_int(15 * n)); zwlr_virtual_pointer_v1_frame(p);
  }
  wl_display_roundtrip(d); usleep(80000);
  zwlr_virtual_pointer_v1_destroy(p);
  wl_display_roundtrip(d);
  wl_display_disconnect(d);
  return 0;
}
