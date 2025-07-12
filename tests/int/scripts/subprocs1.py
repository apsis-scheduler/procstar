"""
Script that forks a bunch of subprocs recursively.
"""

import os
import random
import subprocess
import time


def work():
    pids = set()
    while random.random() < 0.45:
        os.write(1, b"fork\n")
        pid = os.fork()
        if pid > 0:
            pids.add(pid)
            os.write(1, f"forked: {pid}\n".encode())
        else:
            pids = set()
            time.sleep(random.random() * 0.01)
    while len(pids) > 0:
        pid, _ = os.wait()
        os.write(1, f"waited: {pid}\n".encode())
        pids.remove(pid)
    subprocess.run(["/usr/bin/bash", "-c", "echo bash pid is $$"])
    os._exit(0)


pids = set()
for _ in range(10):
    pid = os.fork()
    if pid == 0:
        # Child.
        work()
    else:
        pids.add(pid)

while len(pids) > 0:
    pids.remove(os.wait()[0])

os.write(1, b"done\n")
