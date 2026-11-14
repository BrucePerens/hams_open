# -*- coding: utf-8 -*-
"""Masks personal data before it goes into a log line."""
from email.utils import parseaddr


# [@ANCHOR: hams_base:log_mask_email]
# Verified by [@ANCHOR: test_mask_email_keeps_first_letter_and_domain]
def mask_email(address):
    """j***@example.com: the first character of the local part, then ***, then the domain. "***" for anything else."""
    local, sep, domain = parseaddr(address or "")[1].partition("@")
    if not sep or not local or not domain:
        return "***"
    return f"{local[0]}***@{domain}"
