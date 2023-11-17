"""
JSON-related helpers.
"""

import json

#-------------------------------------------------------------------------------

class Jso:
    """
    Wraps a JSON-deserialized dict to behave like an object.

    Does not check or sanitize keys that are not valid Python identifiers.
    """

    def __init__(self, jso_dict):
        self.__jso_dict = jso_dict


    def __repr__(self):
        args = ", ".join( f"{k}={v!r}" for k, v in self.__jso_dict.items() )
        return f"Object({args})"


    def __getattr__(self, name):
        try:
            val = self.__jso_dict[name]
        except KeyError:
            raise AttributeError(name) from None
        return Jso(val) if isinstance(val, dict) else val


    def __dir__(self):
        return self.__jso_dict.keys()


    @property
    def __dict__(self):
        return self.__jso_dict


    @classmethod
    def wrap(cls, jso):
        """
        Recursively wraps a JSON-deserialized value, if it is an object, and
        any subobjects.
        """
        if isinstance(jso, list):
            return [ cls.wrap(i) for i in jso ]
        elif isinstance(jso, dict):
            return Jso({ k: cls.wrap(v) for k, v in jso.items() })
        else:
            return jso



