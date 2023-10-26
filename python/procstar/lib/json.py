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
        return json.dumps(self.__jso_dict)


    def __getattr__(self, name):
        try:
            val = self.__jso_dict[name]
        except KeyError:
            raise AttributeError(name) from None
        return Jso(val) if isinstance(val, dict) else val


    def __dir__(self):
        return self.__jso_dict.keys()


    def to_jso(self):
        return self.__jso_dict



