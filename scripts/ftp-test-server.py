#!/usr/bin/env python3
"""A real FTP server for the protocol-ftp integration test (needs pyftpdlib).

Usage: TERMALITH_TEST_FTP_PASSWORD=... ftp-test-server.py ROOT PORT
The user is "termalith"; the password is generated per run.
"""
import os
import sys

from pyftpdlib.authorizers import DummyAuthorizer
from pyftpdlib.handlers import FTPHandler
from pyftpdlib.servers import FTPServer


def main() -> None:
    root, port = sys.argv[1], int(sys.argv[2])
    authorizer = DummyAuthorizer()
    authorizer.add_user("termalith", os.environ["TERMALITH_TEST_FTP_PASSWORD"], root, perm="elradfmwMT")
    FTPHandler.authorizer = authorizer
    FTPHandler.passive_ports = range(60000, 60100)
    FTPServer(("127.0.0.1", port), FTPHandler).serve_forever()


if __name__ == "__main__":
    main()
