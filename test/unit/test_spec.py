import procstar.spec


def test_to_jso():
    jso = procstar.spec.Proc(["/usr/bin/sleep"]).to_jso()
    assert jso["argv"] == ("/usr/bin/sleep",)
    assert set(jso.keys())  == {"argv", "env", "fds", "systemd_properties"}
    assert set(jso["systemd_properties"].keys()) == {"scope", "slice"}
