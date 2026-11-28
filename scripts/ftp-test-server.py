#!/usr/bin/env python3
"""A real FTP server for the protocol-ftp integration test (needs pyftpdlib).

Usage: TERMALITH_TEST_FTP_PASSWORD=... ftp-test-server.py ROOT PORT [CERT_PEM [implicit]]
The user is "termalith"; the password is generated per run. With CERT_PEM (a
file with the key and the certificate) the server requires TLS (needs PyOpenSSL);
with "implicit" TLS starts with the connection (implicit FTPS) instead of AUTH TLS.
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
        if len(sys.argv) > 4 and sys.argv[4] == "implicit":

            class ImplicitTLSHandler(TLS_FTPHandler):
                """TLS before the greeting (implicit FTPS)."""

                def handle(self):
                    self.secure_connection(self.ssl_context)

                def handle_ssl_established(self):
                    TLS_FTPHandler.handle(self)

                def ftp_AUTH(self, line):
                    self.respond("534 Already on TLS (implicit FTPS).")

            handler = ImplicitTLSHandler
    handler.authorizer = authorizer
    handler.passive_ports = range(60000, 60100)
    FTPServer(("127.0.0.1", port), handler).serve_forever()


if __name__ == "__main__":
    main()
