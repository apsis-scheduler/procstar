#!/usr/bin/env python

import os

for i in range(256):
    fd = 2 if i % 3 == 0 else 1
    os.write(fd, bytes([i]) * i)
