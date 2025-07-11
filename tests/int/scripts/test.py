#!/usr/bin/env python3

import argparse
import sys

parser = argparse.ArgumentParser()
parser.add_argument(
    "--exit", metavar="CODE", type=int, default=0,
    help="exit with CODE")
args = parser.parse_args()

print("message 0 to stdout")
sys.stdout.flush()
print("message 1 to stderr", file=sys.stderr)
print("message 2 to stdout")

raise SystemExit(args.exit)

