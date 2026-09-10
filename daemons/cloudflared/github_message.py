#!/usr/bin/python3
"""
Create Github Releases Notes with binary checksums from Workers KV
"""

import argparse
import logging
import os
import re
import requests
from urllib.parse import quote

from github import Github, UnknownObjectException

FORMAT = "%(levelname)s - %(asctime)s: %(message)s"
logging.basicConfig(format=FORMAT, level=logging.INFO)

CLOUDFLARED_REPO = os.environ.get("GITHUB_REPO", "cloudflare/cloudflared")
GITHUB_CONFLICT_CODE = "already_exists"
BASE_KV_URL = 'https://api.cloudflare.com/client/v4/accounts/'


# [@ANCHOR: github_message:_raise_for_kv_error]
def _raise_for_kv_error(response, action):
    """
    Raises a clear exception describing a failed Workers KV HTTP call.

    Bug fix (bug-hunt, 2026-09-09): the three call sites that used to inline this check only
    raised when the response body's own "errors" array was both present AND non-empty --  a
    non-200 response with an empty/missing "errors" key (a different error shape, an HTML error
    page, or a malformed body) silently fell through instead of raising at all, letting the
    caller treat garbage/error text as real KV data (see kv_get_value) or crash later with an
    unrelated KeyError (see kv_get_keys). The raised exception itself was also built wrong --
    `Exception("...{0}", errors[0])` passes two positional args instead of formatting the
    string, so the interpolation was silently never applied.
    """
    if response.status_code == 200:
        return
    try:
        errors = response.json().get("errors") or []
    except ValueError:
        errors = []
    detail = errors[0] if errors else response.text
    raise Exception(f"failed to {action}: {detail}")


# [@ANCHOR: github_message:kv_get_keys]
def kv_get_keys(prefix, account, namespace, api_token):
    """ get the KV keys for a given prefix """
    response = requests.get(
        BASE_KV_URL + account + "/storage/kv/namespaces/" + namespace + "/keys",
        headers={
            "Content-Type": "application/json",
            "Authorization": "Bearer " + api_token,
        },
        params={"prefix": prefix},
    )
    _raise_for_kv_error(response, "get checksums")
    return response.json()["result"]


# [@ANCHOR: github_message:kv_get_value]
def kv_get_value(key, account, namespace, api_token):
    """ get the KV value for a provided key """
    response = requests.get(
        BASE_KV_URL + account + "/storage/kv/namespaces/" + namespace + "/values/" + quote(key, safe=""),
        headers={
            "Content-Type": "application/json",
            "Authorization": "Bearer " + api_token,
        },
    )
    _raise_for_kv_error(response, "get checksums")
    return response.text


# [@ANCHOR: github_message:update_or_add_message]
def update_or_add_message(msg, name, sha):
    """
    updates or builds the github version message for each new asset's sha256.
    Searches the existing message string to update or create.

    Bug fix (bug-hunt, 2026-09-09): this used to search for `name` as a bare substring anywhere
    in `msg` (`msg.find(name)`), which conflates an asset's own checksum line with any OTHER
    line that merely contains `name` as a substring -- e.g. "cloudflared-linux-amd64" is a
    literal substring of "cloudflared-linux-amd64-fips" or any other name sharing the same
    prefix. Whether that could actually clobber the wrong line depended entirely on Workers KV's
    list-keys response always being returned in strict lexicographic order (never independently
    verified against Cloudflare's own API contract in this codebase) -- an unverified external
    ordering assumption is exactly the kind of thing that should not be load-bearing for data
    integrity. Fixed to match only a real "name: " entry anchored to the start of a line,
    independent of processing order.
    """
    new_text = '{0}: {1}\n'.format(name, sha)
    pattern = re.compile(r'^' + re.escape(name) + r': .*\n?', re.MULTILINE)
    if pattern.search(msg):
        return pattern.sub(new_text, msg, count=1)
    back = msg.rfind("```")
    if (back != -1):
        return '{0}{1}```'.format(msg[:back], new_text)
    return '{0} \n### SHA256 Checksums:\n```\n{1}```'.format(msg, new_text)


# [@ANCHOR: github_message:get_release]
def get_release(repo, version):
    """ Get a Github Release matching the version tag. """
    try:
        release = repo.get_release(version)
        logging.info("Release %s found", version)
        return release
    except UnknownObjectException:
        logging.info("Release %s not found", version)


# [@ANCHOR: github_message:parse_args]
def parse_args():
    """ Parse and validate args """
    parser = argparse.ArgumentParser(
        description="Updates a Github Release with checksums from KV"
    )
    parser.add_argument(
        "--api-key", default=os.environ.get("API_KEY"), help="Github API key"
    )
    parser.add_argument(
        "--kv-namespace-id", default=os.environ.get("KV_NAMESPACE"), help="workers KV namespace id"
    )
    parser.add_argument(
        "--kv-account-id", default=os.environ.get("KV_ACCOUNT"), help="workers KV account id"
    )
    parser.add_argument(
        "--kv-api-token", default=os.environ.get("KV_API_TOKEN"), help="workers KV API Token"
    )
    parser.add_argument(
        "--release-version",
        metavar="version",
        default=os.environ.get("VERSION"),
        help="Release version",
    )
    parser.add_argument(
        "--dry-run", action="store_true", help="Do not modify the release message"
    )

    args = parser.parse_args()
    is_valid = True
    if not args.release_version:
        logging.error("Missing release version")
        is_valid = False

    if not args.api_key:
        logging.error("Missing API key")
        is_valid = False

    if not args.kv_namespace_id:
        logging.error("Missing KV namespace id")
        is_valid = False

    if not args.kv_account_id:
        logging.error("Missing KV account id")
        is_valid = False

    if not args.kv_api_token:
        logging.error("Missing KV API token")
        is_valid = False

    if is_valid:
        return args

    parser.print_usage()
    exit(1)


# [@ANCHOR: github_message:main]
def main():
    """ Attempts to update the Github Release message with the github asset's checksums """
    try:
        args = parse_args()
        client = Github(args.api_key)
        repo = client.get_repo(CLOUDFLARED_REPO)
        release = get_release(repo, args.release_version)
        if release is None and not args.dry_run:
            # Minor robustness fix (bug-hunt, 2026-09-09): previously this fell through and let
            # `release.update_release(...)` below raise an opaque AttributeError on a NoneType
            # instead of a clear, actionable message.
            raise Exception(f"Release {args.release_version} not found; cannot update message")

        msg = ""

        prefix = f"update_{args.release_version}_"
        keys = kv_get_keys(prefix, args.kv_account_id,
                           args.kv_namespace_id, args.kv_api_token)
        for key in [k["name"] for k in keys]:
            checksum = kv_get_value(
                key, args.kv_account_id, args.kv_namespace_id, args.kv_api_token)
            binary_name = key[len(prefix):]
            msg = update_or_add_message(msg, binary_name, checksum)

        if args.dry_run:
            logging.info("Skipping release message update because of dry-run")
            logging.info(f"Github message:\n{msg}")
            return

        # update the release body text
        release.update_release(args.release_version, msg)

    except Exception as e:
        logging.exception(e)
        exit(1)


main()
