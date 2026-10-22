"""
Test support for the three release scripts in this directory (github_message.py, github_release.py,
release_pkgs.py). They import PyGithub, boto3, botocore and python-gnupg, none of which a dev box
or CI runner for hams.com installs (the scripts only ever run in Cloudflare's own release pipeline),
so `load_script` puts a minimal stand-in for each missing package in sys.modules before importing
the script. A package that IS installed is used as it is.

The stand-ins have only what the scripts import (exception classes and constructors the tests
replace with mocks); no network or GPG call can happen through them.
"""

import importlib.util
import os
import sys
import types

_HERE = os.path.dirname(os.path.abspath(__file__))


class FakeResponse:
    """Just enough of requests.Response: status_code, json(), text."""

    def __init__(self, status_code=200, json_data=None, text=""):
        self.status_code = status_code
        self._json = json_data
        self.text = text

    def json(self):
        if self._json is None:
            raise ValueError("not JSON")
        return self._json


def _have(name):
    try:
        __import__(name)
        return True
    except ImportError:
        return False


def _install_missing_third_party_modules():
    if not _have("github"):
        github = types.ModuleType("github")

        class GithubException(Exception):
            def __init__(self, status=0, data=None, headers=None, message=None):
                super().__init__(status, data)
                self.status = status
                self.data = data or {}

        class UnknownObjectException(GithubException):
            pass

        class Github:
            def __init__(self, *args, **kwargs):
                raise AssertionError("a test must replace Github with a mock; no real GitHub call")

        github.Github = Github
        github.GithubException = GithubException
        github.UnknownObjectException = UnknownObjectException
        sys.modules["github"] = github
    if not _have("boto3"):
        boto3 = types.ModuleType("boto3")

        def client(*args, **kwargs):
            raise AssertionError("a test must replace boto3.client with a mock; no real R2 call")

        boto3.client = client
        sys.modules["boto3"] = boto3
    if not _have("botocore"):
        botocore = types.ModuleType("botocore")
        botocore_client = types.ModuleType("botocore.client")
        botocore_exceptions = types.ModuleType("botocore.exceptions")

        class Config:
            def __init__(self, **kwargs):
                self.kwargs = kwargs

        class ClientError(Exception):
            pass

        botocore_client.Config = Config
        botocore_exceptions.ClientError = ClientError
        botocore.client = botocore_client
        botocore.exceptions = botocore_exceptions
        sys.modules["botocore"] = botocore
        sys.modules["botocore.client"] = botocore_client
        sys.modules["botocore.exceptions"] = botocore_exceptions
    if not _have("gnupg"):
        gnupg = types.ModuleType("gnupg")

        class GPG:
            def __init__(self, *args, **kwargs):
                raise AssertionError("a test must replace gnupg.GPG with a mock; no real gpg call")

        gnupg.GPG = GPG
        sys.modules["gnupg"] = gnupg


def load_script(name):
    """Import <this directory>/<name>.py as a module named 'cloudflared_release_<name>'."""
    _install_missing_third_party_modules()
    module_name = "cloudflared_release_" + name
    if module_name in sys.modules:
        return sys.modules[module_name]
    spec = importlib.util.spec_from_file_location(module_name, os.path.join(_HERE, name + ".py"))
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module
