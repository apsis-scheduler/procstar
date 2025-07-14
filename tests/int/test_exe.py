from procstar.testing.proc import run, run1

# -------------------------------------------------------------------------------


def test_exe():
    # This argv[0] is bogus.
    res = run(
        {
            "specs": {
                "test_exe": {
                    "argv": ["foobar", "Hello, world!"],
                    "fds": [["stdout", {"capture": {"encoding": "utf-8"}}]],
                }
            }
        }
    )
    assert res["test_exe"]["state"] == "error"

    # But we can override it with exe.
    res = run1(
        {
            "exe": "/usr/bin/echo",
            "argv": ["foobar", "Hello, world!"],
            "fds": [["stdout", {"capture": {"encoding": "utf-8"}}]],
        }
    )
    assert res["status"]["status"] == 0
    assert res["fds"]["stdout"]["text"] == "Hello, world!\n"


def test_exe_argv0():
    # Use bash to tell us the argv[0] it sees.
    res = run1(
        {
            "exe": "/usr/bin/bash",
            "argv": ["Not really bash!", "-c", "echo $0"],
            "fds": [["stdout", {"capture": {"encoding": "utf-8"}}]],
        }
    )
    assert res["status"]["status"] == 0
    assert res["fds"]["stdout"]["text"] == "Not really bash!\n"


def test_restrict_exe():
    SPEC = {
        "specs": {
            "test": {
                "argv": ["/usr/bin/echo", "Hello, world!"],
                "fds": [["stdout", {"capture": {"encoding": "utf-8"}}]],
            }
        }
    }

    # Works if the restricted exe matches.
    res = run(SPEC, args=("--restrict-exe", "/usr/bin/echo"))["test"]
    assert res["state"] == "terminated"
    assert res["status"]["status"] == 0
    assert res["fds"]["stdout"]["text"] == "Hello, world!\n"

    # Doesn't work otherwise.
    res = run(SPEC, args=("--restrict-exe", "/usr/bin/true"))["test"]
    assert res["state"] == "error"
    assert "restricted executable" in res["errors"][0]


def test_restrict_exe_multi():
    SPEC = {
        "specs": {
            "sleep": {
                "argv": ["/usr/bin/sleep", "0"],
            },
            "echo": {
                "argv": ["/usr/bin/echo", "Hello, world!"],
                "fds": [["stdout", {"capture": {"encoding": "utf-8"}}]],
            },
            "true": {
                "argv": ["/usr/bin/true"],
            },
        }
    }

    res = run(SPEC)
    for proc_id in SPEC["specs"]:
        assert res[proc_id]["state"] == "terminated"
        assert res[proc_id]["status"]["status"] == 0

    # Works iff the restricted exe matches.
    res = run(SPEC, args=("--restrict-exe", "/usr/bin/echo"))
    assert res["sleep"]["state"] == "error"
    assert res["echo"]["state"] == "terminated"
    assert res["echo"]["status"]["status"] == 0
    assert res["echo"]["fds"]["stdout"]["text"] == "Hello, world!\n"
    assert res["true"]["state"] == "error"

    res = run(SPEC, args=("--restrict-exe", "/usr/bin/true"))
    assert res["sleep"]["state"] == "error"
    assert res["echo"]["state"] == "error"
    assert res["true"]["state"] == "terminated"
    assert res["true"]["status"]["status"] == 0
