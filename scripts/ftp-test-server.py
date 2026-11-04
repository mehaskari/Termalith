#!/usr/bin/env python3
"""A real FTP server for the protocol-ftp integration test (needs pyftpdlib).

Usage: TERMALITH_TEST_FTP_PASSWORD=... ftp-test-server.py ROOT PORT [CERT_PEM]
The user is "termalith"; the password is generated per run. With CERT_PEM (a
file with the key and the certificate) the server requires TLS (needs PyOpenSSL).
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
    handler = FTPHandler
    if len(sys.argv) > 3:
        from pyftpdlib.handlers import TLS_FTPHandler

        handler = TLS_FTPHandler
        handler.certfile = sys.argv[3]
        handler.tls_control_required = True
        handler.tls_data_required = True
    handler.authorizer = authorizer
    handler.passive_ports = range(60000, 60100)
    FTPServer(("127.0.0.1", port), handler).serve_forever()


if __name__ == "__main__":
    main()
