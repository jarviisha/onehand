#!/usr/bin/env python3
"""A single bounded output burst, followed by a drain window and idle."""
import os
from pathlib import Path
import subprocess
import time

phase_file = Path(os.environ['PERF_PHASE'])


def phase(name):
    size = os.get_terminal_size()
    phase_file.write_text(f'{name}\n{size.columns}\n{size.lines}\n')


phase('warmup')
time.sleep(3)
phase('seq-window')
subprocess.run(['seq', '1', '200000'], check=True)
print('ONEHAND_SEQ_DONE', flush=True)
time.sleep(8)
phase('idle')
time.sleep(5)
phase('done')
# Keep the final grid visible for manual inspection or an external sampler.
time.sleep(60)
