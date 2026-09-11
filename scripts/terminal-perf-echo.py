#!/usr/bin/env python3
"""Acknowledge simulated probe keystrokes through a real raw PTY.

Every input emits exactly one numbered update. The echo-redraw phase also
rewrites a dense 96x50 area. Trace timing can then pair input -> batch -> paint
-> GPUI submission, without attributing an unrelated background frame to echo.
This measures the app dispatch path, not physical keyboard/compositor latency.
"""
import os
from pathlib import Path
import termios
import time
import tty

phase_path = Path(os.environ['PERF_PHASE'])
count = int(os.getenv('PERF_HZ', '20')) * 8


def phase(name):
    size = os.get_terminal_size()
    temp = phase_path.with_suffix('.tmp')
    temp.write_text(f'{name}\n{size.columns}\n{size.lines}\n')
    temp.replace(phase_path)


def write_all(data):
    while data:
        data = data[os.write(1, data):]


saved = termios.tcgetattr(0)
try:
    tty.setraw(0)
    phase('warmup')
    time.sleep(3)
    write_all(b'\x1b[?25l\x1b[2J')
    screen = ''.join(f'\x1b[{row};1H' + 'source text ' * 8 for row in range(1, 51)).encode()
    for name in ('echo', 'echo-redraw'):
        phase(name)
        for tick in range(count):
            assert os.read(0, 1) == b'a', 'unexpected input to isolated echo probe'
            write_all((screen if name == 'echo-redraw' else b'') + f'\x1b[H{tick:04}'.encode())
    phase('idle')
    time.sleep(4)
    phase('done')
    time.sleep(60)
finally:
    termios.tcsetattr(0, termios.TCSANOW, saved)
