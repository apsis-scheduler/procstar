import contextlib
import json
import requests
from   urllib.parse import quote, quote_plus, urlunsplit
import uuid

#-------------------------------------------------------------------------------

class AsyncClient:

    def __init__(self, addr, http_client):
        host, port          = addr
        self.__host         = str(host)
        self.__port         = int(port)
        self.__http_client  = http_client


    @contextlib.asynccontextmanager
    async def __request(self, method, *path, jso=None, args={}):
        netloc = f"{self.__host}:{self.__port}"
        path = "/" + "/".join( quote(p, safe="") for p in path )
        query = "&".join( f"{k}={quote_plus(v)}" for k, v in args.items() )
        url = urlunsplit((
            "http",     # FIXME: HTTPS
            netloc,
            path,
            query,
            "",         # fragment
        ))
        headers = {}
        content = None
        if jso is not None:
            headers["content-type"] = "application/json"
            content = json.dumps(jso)

        # FIXME: Handle errors.
        rsp = await self.__http_client.request(
            method, url,
            headers=headers,
            content=content,
        )
        yield rsp
        # FIXME: Close rsp?


    async def post_proc(self, spec, *, proc_id=None):
        if proc_id is None:
            proc_id = str(uuid.uuid4())
        jso = {
            "specs": {
                proc_id: spec.to_jso(),
            }
        }

        async with self.__request("POST", "procs", jso=jso) as rsp:
            rsp.raise_for_status()  # FIXME

            assert rsp.headers["content-type"] == "application/json"
            # FIXME: Get proc ID from response.

        return proc_id


    async def get_proc(self, proc_id):
        async with self.__request("GET", "procs", proc_id) as rsp:
            rsp.raise_for_status()  # FIXME
            return rsp.json()["data"]["procs"][proc_id]


    async def delete_proc(self, proc_id):
        async with self.__request("DELETE", "procs", proc_id) as rsp:
            rsp.raise_for_status()


    async def post_signal(self, proc_id, signum):
        async with self.__request("POST", "procs", proc_id, "signals", signum) as rsp:
            rsp.raise_for_status()



#-------------------------------------------------------------------------------

class Client:

    def __init__(self, addr):
        host, port      = addr
        self.__host     = str(host)
        self.__port     = int(port)
        session = requests.Session()
        session.mount(
            "http://",
            requests.adapters.HTTPAdapter(
                max_retries=requests.adapters.Retry(total=5, backoff_factor=1),
            )
        )
        self.__session  = session


    def __request(self, method, *path, jso=None, args={}):
        netloc = f"{self.__host}:{self.__port}"
        path = "/" + "/".join( quote(p, safe="") for p in path )
        query = "&".join( f"{k}={quote_plus(v)}" for k, v in args.items() )
        url = urlunsplit((
            "http",     # FIXME: HTTPS
            netloc,
            path,
            query,
            "",         # fragment
        ))
        headers = {}

        return self.__session.request(
            method, url,
            headers=headers,
            json=jso,
        )


    def get_procs(self):
        with self.__request("GET", "procs") as rsp:
            rsp.raise_for_status()
            return rsp.json()["data"]["procs"]



