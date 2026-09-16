#!/usr/bin/env python3
"""Compare retained and fresh terminal renderers in an isolated Hyprland window.

Requires the terminal_cache example, grim and Pillow. Captures only its own
client area, checks all 17 cases, and restores the previous focus. Do
not run alongside performance measurements. A one-step RGB tolerance allows
position-dependent 8-bit rounding; larger differences fail the comparison.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import time

from PIL import Image, ImageChops


def hypr(*args):
    return subprocess.check_output(['hyprctl', *args], text=True)


def client(pid):
    return next((c for c in json.loads(hypr('clients', '-j')) if c['pid'] == pid), None)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('name')
    parser.add_argument('--binary', type=Path, default=Path('target/debug/examples/terminal_cache'))
    parser.add_argument('--output', type=Path, default=Path('/tmp/onehand-terminal-cache'))
    parser.add_argument('--monitor', default='eDP-1')
    parser.add_argument('--x11-scale', type=float, choices=(1.0, 1.25, 1.5, 2.0))
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    step = args.output / f'{args.name}.step'
    if step.exists():
        parser.error('use a fresh case name; artifacts are never overwritten')
    step.write_text('0')
    log_path = args.output / f'{args.name}.log'
    scale = args.x11_scale or 1.0
    env = dict(os.environ, PERF_CACHE_STEP=str(step.resolve()))
    if args.x11_scale:
        env.pop('WAYLAND_DISPLAY', None)
        env['GPUI_X11_SCALE_FACTOR'] = str(scale)
    focused = json.loads(hypr('activewindow', '-j')).get('address')
    monitor = next(m for m in json.loads(hypr('monitors', '-j')) if m['name'] == args.monitor)
    results = []
    with log_path.open('w') as log:
        proc = subprocess.Popen([str(args.binary.resolve())], env=env, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 15
            while not (window := client(proc.pid)):
                if proc.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError(f'comparison window did not open: {log_path}')
                time.sleep(.1)
            address = 'address:' + window['address']
            hypr('dispatch', 'setfloating', address)
            hypr('dispatch', 'movetoworkspacesilent', f"{monitor['activeWorkspace']['id']},{address}")
            hypr('dispatch', 'resizewindowpixel', f'exact {round(896 * scale)} {round(320 * scale)},{address}')
            hypr('dispatch', 'movewindowpixel', f"exact {monitor['x'] + 10} {monitor['y'] + 60},{address}")
            hypr('dispatch', 'focuswindow', address)
            time.sleep(2)
            for case in range(17):
                step.write_text(str(case))
                deadline = time.monotonic() + 5
                while True:
                    records = re.findall(rf'cache_comparison case={case} fresh=(true|false) scale=([\d.]+)',
                                         log_path.read_text())
                    if {fresh for fresh, actual in records if float(actual) == scale} == {'true', 'false'}:
                        break
                    if time.monotonic() > deadline:
                        raise RuntimeError(f'case {case} was not painted at scale {scale}')
                    time.sleep(.1)
                time.sleep(.8)
                window = client(proc.pid)
                if not window or not window['visible'] or window['hidden']:
                    raise RuntimeError('comparison window is not visible')
                x, y = window['at']
                w, h = window['size']
                if [w, h] != [round(896 * scale), round(320 * scale)]:
                    raise RuntimeError('comparison window changed size')
                path = args.output / f'{args.name}-{case:02}.png'
                subprocess.run(['grim', '-g', f'{x},{y} {w}x{h}', str(path)], check=True)
                im = Image.open(path).convert('RGB')
                inset, half, bottom = round(8 * scale), round(448 * scale), round(312 * scale)
                left = im.crop((inset, inset, half - inset, bottom))
                right = im.crop((half + inset, inset, 2 * half - inset, bottom))
                diff = ImageChops.difference(left, right)
                pixels = getattr(diff, 'get_flattened_data', diff.getdata)()
                values = [max(pixel) for pixel in pixels]
                results.append({'case': case, 'different_pixels': sum(v > 0 for v in values),
                                'max_channel_difference': max(values),
                                'pixels_over_tolerance_1': sum(v > 1 for v in values),
                                'compared_pixels': left.width * left.height, 'window_size': [w, h]})
            (args.output / f'{args.name}.json').write_text(json.dumps(results, indent=2) + '\n')
            if any(r['pixels_over_tolerance_1'] for r in results):
                raise RuntimeError('retained and fresh renderers differ; see captured images')
            print(f'{args.name}: all 17 cases agree within one RGB step')
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
            if focused:
                hypr('dispatch', 'focuswindow', 'address:' + focused)


if __name__ == '__main__':
    main()
