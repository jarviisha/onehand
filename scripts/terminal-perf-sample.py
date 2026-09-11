#!/usr/bin/env python3
"""Sample host CPU and Linux DRM engine time without sampling the child/editor.

Usage: terminal-perf-sample.py PID --seconds 10
GPU percentages are per engine, derived from cumulative nanoseconds, not VRAM.
Duplicate file descriptors for one DRM client are counted only once. Unsupported
drivers report an empty engine map, never a fabricated zero GPU measurement.
"""

import argparse
import json
import os
from pathlib import Path
import time


def snapshot(pid):
    proc = Path('/proc') / str(pid)
    # comm can contain spaces and parentheses; fields after its final ')' start
    # at field 3. utime/stime are fields 14/15, RSS is field 24.
    fields = (proc / 'stat').read_text().rsplit(')', 1)[1].split()
    engines = {}
    clients = set()
    for fd in (proc / 'fdinfo').iterdir():
        try:
            info = dict(line.split(':', 1) for line in fd.read_text().splitlines() if ':' in line)
        except (FileNotFoundError, PermissionError):
            continue
        client = info.get('drm-client-id')
        if client is None:
            continue
        key = (info.get('drm-pdev'), client)
        if key in clients:
            continue
        clients.add(key)
        for name, value in info.items():
            if name.startswith('drm-engine-'):
                parts = value.split()
                if len(parts) == 2 and parts[1] == 'ns':
                    count = parts[0]
                    engine = f"{info.get('drm-pdev', 'unknown').strip()}/{name[11:]}"
                    engines[engine] = engines.get(engine, 0) + int(count)
    return {
        'time': time.monotonic(),
        'cpu_seconds': (int(fields[11]) + int(fields[12])) / os.sysconf('SC_CLK_TCK'),
        'rss_bytes': int(fields[21]) * os.sysconf('SC_PAGE_SIZE'),
        'engines_ns': engines,
    }


def difference(before, after):
    elapsed = after['time'] - before['time']
    cpu = after['cpu_seconds'] - before['cpu_seconds']
    engines = {
        key: (value - before['engines_ns'][key]) / 1e9
        for key, value in after['engines_ns'].items()
        if key in before['engines_ns'] and value >= before['engines_ns'][key]
    }
    return {
        'wall_seconds': elapsed,
        'cpu_seconds': cpu,
        'cpu_percent_one_core': 100 * cpu / elapsed,
        'gpu_engine_seconds': engines,
        'gpu_engine_percent': {key: 100 * value / elapsed for key, value in engines.items()},
        'rss_bytes': after['rss_bytes'],
    }


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('pid', type=int)
    parser.add_argument('--seconds', type=float, default=10)
    args = parser.parse_args()
    if args.seconds <= 0:
        parser.error('--seconds must be positive')
    before = snapshot(args.pid)
    time.sleep(args.seconds)
    print(json.dumps(difference(before, snapshot(args.pid)), indent=2))
