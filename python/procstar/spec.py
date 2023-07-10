
#-------------------------------------------------------------------------------

class Proc:

    class Env:

        def __init__(self, *, inherit=True, vars={}):
            self.__inherit = (
                inherit if isinstance(inherit, bool)
                else tuple( str(n) for n in inherit )
            )
            self.__vars = { str(n): str(v) for n, v in vars.items() }


        def to_jso(self):
            return {
                "inherit": self.__inherit,
                "vars": dict(self.__vars),
            }



    class Fd:

        class Inherit:

            def to_jso(self):
                return {
                    "inherit": {},
                }



        class Close:

            def to_jso(self):
                return {
                    "close": {},
                }



        class Null:

            def __init__(self, flags="Default"):
                self.__flags = flags  # FIXME: Validate.


            def to_jso(self):
                return {
                    "null": {
                        "flags": self.__flags,
                    }
                }



        # FIXME
        # class File

        class Dup:

            def __init__(self, fd):
                self.__fd = fd  # FIXME: Validate.


            def to_jso(self):
                return {
                    "dup": {
                        "fd": self.__fd,
                    }
                }



        class Capture:

            def __init__(self, mode, format):
                # FIXME: Validate.
                self.__mode = mode
                self.__format = format


            def to_jso(self):
                return {
                    "capture": {
                        "mode": self.__mode,
                        "format": self.__format,
                    }
                }



    def __init__(self, argv, *, env=Env(), fds=[]):
        self.__argv = tuple( str(a) for a in argv )
        self.__env  = env
        self.__fds  = dict(fds)


    def to_jso(self):
        return {
            "argv"  : self.__argv,
            "env"   : self.__env.to_jso(),
            "fds"   : [ (n, f.to_jso()) for n, f in self.__fds.items() ],
        }



