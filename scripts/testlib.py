#!/usr/bin/env python3

import glob
import os
import subprocess
from concurrent.futures import ThreadPoolExecutor, as_completed
from multiprocessing import cpu_count
import fcntl
import re


fcntl.fcntl(1, fcntl.F_SETFL, 0)


current_path = os.path.dirname(os.path.abspath(__file__))
target_debug = os.path.abspath(os.path.join(current_path, '../target/debug'))


def clean_binary_tests():
    for f in glob.glob(f'{target_debug}/*'):
        if os.path.isfile(f):
            os.remove(f)


def build_tests():
    p = subprocess.run(['cargo', 'test', '--workspace', '--no-run'])
    if p.returncode != 0:
        os._exit(p.returncode)


def workers():
    workers = cpu_count() // 2
    print(f'========= run in {workers} workers')
    return workers


def test_binaries(exclude=None):
    binaries = []
    for f in glob.glob(f'{target_debug}/*'):
        fname = os.path.basename(f)
        ext = os.path.splitext(fname)[1]
        if os.path.isfile(f) and fname != 'near' and ext == '':
            if not exclude:
                binaries.append(f)
            elif not any(map(lambda e: re.match(e, fname), exclude)):
                binaries.append(f)
            else:
                print(f'========= ignore {f}')
    return binaries


def run_test(test_binary, isolate=True):
    """ Run a single test, save exitcode, stdout and stderr """
    if isolate:
        cmd = ['docker', 'run', '--rm',
               '-v', f'{test_binary}:{test_binary}',
               'ailisp/near-test-runtime',
               'bash', '-c', f'chmod +x {test_binary} && RUST_BACKTRACE=1 {test_binary}']
    else:
        cmd = [test_binary]
    p = subprocess.Popen(cmd,
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE, universal_newlines=True)
    stdout, stderr = p.communicate()
    return (p.returncode, stdout, stderr)