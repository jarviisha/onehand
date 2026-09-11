#!/usr/bin/env python3
"""Replay identical, fixed-area ANSI updates at an absolute deadline cadence.

PERF_HZ defaults to 20; PERF_SECONDS to 8 per active phase. PERF_BORDER selects
text, straight or rounded. No terminal replies or search results affect bytes.
The companion .events.json records payload hashes, deadlines and bytes written;
it proves equal offered work, not that the compositor presented every update.
"""

import hashlib
import json
import os
from pathlib import Path
import shutil
import time


def screen(border):
    # Two panels in a fixed 100 x 50 area, independent of the available grid.
    corners = {'text': (' ', ' ', ' ', ' '),
               'straight': ('┌', '┐', '└', '┘'),
               'rounded': ('╭', '╮', '╰', '╯')}[border]
    vertical, horizontal = (' ', ' ') if border == 'text' else ('│', '─')
    rows = []
    for row in range(50):
        panels = []
        for panel in range(2):
            if row == 0:
                value = corners[0] + horizontal * 48 + corners[1]
            elif row == 49:
                value = corners[2] + horizontal * 48 + corners[3]
            else:
                text = f' local value_{row:03} = compute({panel}, "source")'
                value = vertical + text.ljust(48) + vertical
            panels.append(value)
        rows.append(f'\x1b[{row + 1};1H' + ''.join(panels))
    return ''.join(rows).encode()


def write_all(payload):
    while payload:
        payload = payload[os.write(1, payload):]


def main():
    phase_file = Path(os.environ['PERF_PHASE'])
    hz = float(os.getenv('PERF_HZ', '20'))
    seconds = float(os.getenv('PERF_SECONDS', '8'))
    assert 0 < hz <= 120 and seconds >= 2
    count = round(hz * seconds)
    border = os.getenv('PERF_BORDER', 'straight')
    content = screen(border)
    events = []

    def phase(name):
        cols, rows = shutil.get_terminal_size()
        temp = phase_file.with_suffix('.tmp')
        temp.write_text(f'{name}\n{cols}\n{rows}\n')
        temp.replace(phase_file)

    phase('warmup')
    time.sleep(3)
    cols, rows = shutil.get_terminal_size()
    if cols < 100 or rows < 50:
        raise RuntimeError(f'need at least 100x50 cells, got {cols}x{rows}')
    write_all(b'\x1b[?1049h\x1b[?25l\x1b[0m\x1b[2J'
              b'\x1b[38;2;216;222;233m\x1b[48;2;32;37;48m' + content)
    time.sleep(2)
    for name in ('small', 'full'):
        phase(name)
        start = time.monotonic()
        digest = hashlib.sha256()
        byte_count = 0
        lateness = []
        for tick in range(count):
            deadline = start + tick / hz
            time.sleep(max(0, deadline - time.monotonic()))
            lateness.append(time.monotonic() - deadline)
            status = f'\x1b[25;10H{tick % 10}'.encode()
            payload = (content if name == 'full' else b'') + status
            write_all(payload)
            digest.update(payload)
            byte_count += len(payload)
        time.sleep(max(0, start + seconds - time.monotonic()))
        events.append({'phase': name, 'updates': count, 'bytes': byte_count,
                       'payload_sha256': digest.hexdigest(), 'hz': hz,
                       'elapsed_seconds': time.monotonic() - start,
                       'max_deadline_lateness_ms': max(lateness) * 1000})
    phase('idle')
    time.sleep(4)
    phase_file.with_suffix('.events.json').write_text(json.dumps(events, indent=2) + '\n')
    phase('done')
    # Keep the same contents for the runner's final sample and inspection.
    time.sleep(60)


if __name__ == '__main__':
    main()
