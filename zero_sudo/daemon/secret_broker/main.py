#!/usr/bin/env python3
# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Entry point of the Tier B secret broker (hams-secret-broker.service). See broker.py.

    main.py --config /etc/hams_secret_broker/broker.json

Startup refuses, with a message naming the problem and never a value: running as root, a config that does
not validate, an unknown account, a credentials source the web account could read, and a configured secret
that is missing or shorter than 8 characters.
"""

import argparse
import json
import logging
import os
import sys

_HERE = os.path.dirname(os.path.abspath(__file__))
sys.path[:0] = [_HERE, os.path.dirname(_HERE)]

import broker  # noqa: E402
import secret_store  # noqa: E402


# [@ANCHOR: zero_sudo:secret_broker_main_build]
def build(config, environ, euid, ids=broker.account_ids):
    """The Broker for a loaded config, after every startup check (see the module docstring)."""
    if euid == 0:
        raise broker.BrokerConfigError("the broker must not run as root; run it as its own account")
    broker.validate_config(config)
    allowed = {ids(user)[0] for user in config["allowed_users"]}
    if config["credentials"] == "systemd":
        directory = environ.get("CREDENTIALS_DIRECTORY")
        if not directory:
            raise broker.BrokerConfigError("credentials 'systemd' needs LoadCredential= in the unit")
        path = os.path.join(directory, "secrets")
        # systemd made this per-service copy (root-owned with an ACL, or owned by the service account,
        # depending on the systemd version); the owner check that matters is on the root:root 0600 source.
        backend = secret_store.CredentialsFileBackend(path, owner_uid=os.stat(path).st_uid)
        paths = [path]
    else:
        backend = secret_store.CredentialsFileBackend(config["credentials"])
        paths = [config["credentials"]]
    broker.check_not_readable(paths, config["deny_readers"], ids)
    instance = broker.Broker(config, secret_store.SecretStore([backend]), allowed)
    instance.check_secrets_present()
    return instance


# [@ANCHOR: zero_sudo:secret_broker_main]
def main(argv=None):
    """Parse arguments, build the broker, serve until stopped. Exit status 2 on a startup refusal."""
    parser = argparse.ArgumentParser(description="Tier B secret broker")
    parser.add_argument("--config", default="/etc/hams_secret_broker/broker.json")
    args = parser.parse_args(argv)
    logging.basicConfig(level=logging.INFO, format="%(levelname)s %(message)s")
    try:
        with open(args.config, encoding="utf-8") as handle:
            config = json.load(handle)
        instance = build(config, os.environ, os.geteuid())
    except (OSError, ValueError, secret_store.SecretStoreError) as exc:
        logging.error("secret broker refuses to start: %s", exc)
        return 2
    instance.serve()
    return 0


if __name__ == "__main__":
    sys.exit(main())
