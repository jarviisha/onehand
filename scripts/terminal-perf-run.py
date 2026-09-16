#!/usr/bin/env python3
"""Run one visible Hyprland terminal benchmark and retain its measurements.

Launches and controls only its own window, restoring focus afterwards. Requires
host /proc DRM access, Hyprland IPC, and a prebuilt probe (or Kitty). Run cases
sequentially, with no compilation in progress. Geometry/visibility changes
invalidate a run; desktop occlusion and dynamic GPU clocks still need care.
"""

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import statistics
import subprocess
import sys
import time

sys.dont_write_bytecode = True
SCRIPTS = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('sampler', SCRIPTS / 'terminal-perf-sample.py')
sampler = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sampler)


def hypr(*args):
    return subprocess.check_output(['hyprctl', *args], text=True)


def client(pid):
    return next((c for c in json.loads(hypr('clients', '-j')) if c['pid'] == pid), None)


def geometry(window):
    return {key: window[key] for key in ('at', 'size', 'monitor', 'mapped', 'visible', 'hidden', 'workspace')}


def read_log(path):
    records = []
    content = path.read_text(errors='replace')
    # Ignore a final line whose producer has not finished writing it yet.
    for line in content[:content.rfind('\n') + 1].splitlines():
        if line.startswith('sample_s='):
            record = {key: float(value) for key, value in re.findall(r'(\w+)=([\d.]+)', line)}
            record['stages'] = {}
            records.append(record)
        elif line.startswith('terminal_stage ') and records:
            parts = dict(part.split('=', 1) for part in line.split()[1:])
            stage = parts.pop('stage')
            records[-1]['stages'][stage] = {k: int(v) for k, v in parts.items()}
        elif line.startswith('terminal_cache ') and records:
            records[-1]['cache'] = {key: int(value) for key, value in re.findall(r'(\w+)=(\d+)', line)}
    return records


def gpu_clocks():
    clocks = {}
    for path in Path('/sys/class/drm').glob('card[0-9]*/device/pp_dpm_sclk'):
        try:
            match = re.search(r'(\d+)Mhz\s+\*', path.read_text())
            if match:
                clocks[path.parent.resolve().name] = int(match[1])
        except OSError:
            pass
    return clocks


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('name')
    parser.add_argument('--host', choices=('probe', 'kitty'), default='probe')
    parser.add_argument('--binary', type=Path, default=SCRIPTS.parent / 'target/release/examples/terminal_perf')
    parser.add_argument('--output', type=Path, default=Path('/tmp/onehand-terminal-profiling'))
    parser.add_argument('--workload', choices=('replay', 'cursor', 'snacks', 'seq', 'echo'), default='replay')
    parser.add_argument('--hz', type=int, default=20)
    parser.add_argument('--app', choices=('terminal', 'neovim'), help='mount the actual application dock or Workbench')
    parser.add_argument('--project', type=Path, help='isolated project root for full-application cases')
    parser.add_argument('--user-nvim-config', action='store_true', help='load the installed user Neovim configuration')
    parser.add_argument('--trace', action='store_true', help='enable bounded PTY batch/render event logs')
    parser.add_argument('--monitor', default='eDP-1', help='Hyprland monitor name for the isolated window')
    parser.add_argument('--border', choices=('text', 'straight', 'rounded'), default='straight')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    phase_path = args.output / f'{args.name}.phase'
    log_path = args.output / f'{args.name}.log'
    result_path = args.output / f'{args.name}.json'
    if result_path.exists() or phase_path.exists():
        parser.error('use a fresh case name; measurements are never overwritten')
    if args.hz not in (20, 40):
        parser.error('--hz must be 20 or 40')
    if args.workload == 'echo' and (args.host != 'probe' or args.app):
        parser.error('echo uses the standalone GPUI probe input injector')
    env = dict(os.environ, PERF_PHASE=str(phase_path), PERF_WORKLOAD=args.workload,
               PERF_HZ=str(args.hz), PERF_BORDER=args.border,
               PERF_INTERVAL_MS=str(1000 // args.hz), PERF_TICKS=str(args.hz * 8),
               PERF_GUIDES='1', SHELL=str(SCRIPTS / 'terminal-perf-workload.sh'))
    if args.app:
        if args.host != 'probe' or args.project is None:
            parser.error('--app requires --host probe and --project')
        if args.app == 'neovim' and args.workload not in ('cursor', 'snacks'):
            parser.error('Workbench acceptance supports cursor or snacks workloads')
        app_config = args.output / f'{args.name}.config'
        (app_config / 'onehand').mkdir(parents=True)
        (app_config / 'onehand/config.toml').write_text('agents = []\n[font]\nmonospace = "Liberation Mono"\n')
        env.update(PERF_APP=args.app, XDG_CONFIG_HOME=str(app_config.resolve()))
        if args.app == 'neovim':
            real_nvim = shutil.which('nvim')
            if not real_nvim:
                parser.error('Neovim is not installed')
            wrappers = args.output / f'{args.name}.bin'
            wrappers.mkdir()
            wrapper = wrappers / 'nvim'
            # Only a bare launch from Program::Neovim runs the workload. Any
            # nested nvim invocation from user plugins retains its own arguments.
            wrapper.write_text('#!/bin/sh\n'
                               'export XDG_CONFIG_HOME="$PERF_NVIM_CONFIG_HOME"\n'
                               'if [ "$#" -gt 0 ]; then exec "$PERF_NVIM" "$@"; fi\n'
                               'exec ' + shlex.quote(str(SCRIPTS / 'terminal-perf-workload.sh')) + '\n')
            wrapper.chmod(0o755)
            env.update(PERF_NVIM=real_nvim, PERF_NVIM_CONFIG_HOME=os.environ.get('XDG_CONFIG_HOME', str(Path.home() / '.config')),
                       PATH=str(wrappers.resolve()) + os.pathsep + os.environ['PATH'])
    if args.user_nvim_config:
        if args.app != 'neovim':
            parser.error('--user-nvim-config requires --app neovim')
        env['PERF_USER_NVIM_CONFIG'] = '1'
    if args.trace:
        env['ONEHAND_TERMINAL_TRACE'] = '1'
    command = [str(args.binary.resolve())] if args.host == 'probe' else [
        'kitty', '--config', 'NONE', '--class', 'onehand-terminal-perf-kitty',
        '-o', 'font_family=Liberation Mono', '-o', 'font_size=10.5',
        '-o', 'cursor_blink_interval=0', '-o', 'window_padding_width=0', env['SHELL']]
    if args.app:
        command.append(str(args.project.resolve()))
    focused = json.loads(hypr('activewindow', '-j')).get('address')
    monitor = next(m for m in json.loads(hypr('monitors', '-j')) if m['name'] == args.monitor)
    samples, profiles, windows = [], [], []
    invalid = []
    with log_path.open('w') as log:
        proc = subprocess.Popen(command, env=env, stdout=log, stderr=log, start_new_session=True)
        try:
            deadline = time.monotonic() + 20
            while not (window := client(proc.pid)):
                if proc.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError(f'benchmark did not open; see {log_path}')
                time.sleep(.1)
            address = 'address:' + window['address']
            hypr('dispatch', 'setfloating', address)
            hypr('dispatch', 'movetoworkspacesilent', f"{monitor['activeWorkspace']['id']},{address}")
            hypr('dispatch', 'resizewindowpixel', f'exact 960 1000,{address}')
            hypr('dispatch', 'movewindowpixel', f"exact {monitor['x'] + 10} {monitor['y'] + 60},{address}")
            if not window['pinned']:
                hypr('dispatch', 'pin', address)
            hypr('dispatch', 'focuswindow', address)
            time.sleep(.5)
            expected = geometry(client(proc.pid))
            if (expected['monitor'] != monitor['id'] or expected['hidden']
                    or not expected['mapped'] or not expected['visible']
                    or any(abs(a - b) > 1 for a, b in zip(expected['size'], (960, 1000)))):
                raise RuntimeError(f'benchmark window is not ready: {expected}')
            deadline = time.monotonic() + 150
            previous_profile = None
            next_geometry = 0
            while time.monotonic() < deadline:
                phase = (phase_path.read_text().splitlines() if phase_path.exists() else []) or ['startup']
                sample = sampler.snapshot(proc.pid)
                sample['gpu_clock_mhz'] = gpu_clocks()
                sample['phase'] = phase[0]
                sample['grid'] = phase[1:3]
                samples.append(sample)
                records = read_log(log_path)
                if records and records[-1]['sample_s'] != previous_profile:
                    # A complete profiling sample has all eight stage lines.
                    record = records[-1]
                    if len(record['stages']) in (0, 8):
                        record.update(time=sample['time'], phase=phase[0])
                        profiles.append(record)
                        previous_profile = record['sample_s']
                if sample['time'] >= next_geometry:
                    current = client(proc.pid)
                    measured = geometry(current) if current else None
                    windows.append({'time': sample['time'], 'geometry': measured})
                    if measured != expected:
                        invalid.append('window geometry/visibility changed')
                    next_geometry = sample['time'] + .5
                if phase[0] == 'done':
                    break
                time.sleep(.05)
            else:
                invalid.append('workload did not finish')
            phases = {}
            for phase in dict.fromkeys(s['phase'] for s in samples):
                points = [s for s in samples if s['phase'] == phase]
                if len(points) < 2:
                    continue
                result = sampler.difference(points[0], points[-1])
                result['grid'] = points[-1]['grid']
                # Exclude startup/drain boundaries for per-update attribution.
                start, end = points[0]['time'] + 1, points[-1]['time'] - 1
                steady = [s for s in points if start <= s['time'] <= end]
                if len(steady) >= 2:
                    result['steady'] = sampler.difference(steady[0], steady[-1])
                    result['steady']['gpu_clock_mhz'] = {
                        device: {'min': min(values), 'median': statistics.median(values), 'max': max(values)}
                        for device in {key for s in steady for key in s['gpu_clock_mhz']}
                        if (values := [s['gpu_clock_mhz'][device] for s in steady if device in s['gpu_clock_mhz']])}
                stats = [s for s in profiles if s['phase'] == phase and start <= s['time'] <= end]
                if len(stats) >= 2:
                    before, after = stats[0], stats[-1]
                    result['profile'] = {
                        'wall_seconds': after['time'] - before['time'],
                        'frames': after['frames'] - before['frames'],
                        'draw_ns': after['draw_ns'] - before['draw_ns'],
                        'stages': {stage: {key: value - before['stages'][stage][key]
                                          for key, value in values.items()}
                                   for stage, values in after['stages'].items()
                                   if stage in before['stages']}}
                    if 'cache' in before and 'cache' in after:
                        result['profile']['cache'] = {
                            key: value - before['cache'][key] for key, value in after['cache'].items()}
                phases[phase] = result
            events_path = phase_path.with_suffix('.events.json')
            result = {'name': args.name, 'host': args.host, 'workload': args.workload,
                      'hz': args.hz, 'border': args.border, 'window': expected,
                      'app': args.app, 'user_nvim_config': args.user_nvim_config,
                      'invalid': sorted(set(invalid)), 'phases': phases,
                      'events': json.loads(events_path.read_text()) if events_path.exists() else [],
                      'binary_sha256': hashlib.sha256(Path(command[0]).read_bytes()).hexdigest()
                      if args.host == 'probe' else None}
            result_path.write_text(json.dumps(result, indent=2) + '\n')
            (args.output / f'{args.name}.raw.json').write_text(json.dumps({
                'samples': samples, 'profiles': profiles, 'windows': windows}) + '\n')
            print(json.dumps(result, indent=2), flush=True)
            if invalid:
                raise RuntimeError('invalid run; retain artifacts but exclude from comparisons')
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
