# SPDX-License-Identifier: AGPL-3.0-or-later
"""What a controller answers when the shield refuses a submission, for forms that redirect."""
from odoo.http import request

from .models.form_shield import (
    LIMITED,
    QUESTION,
    SILENT,
    UNAVAILABLE,
)

PAGE_STATUS = {LIMITED: 429, UNAVAILABLE: 503}


# [@ANCHOR: form_shield:with_query]
# Verified by [@ANCHOR: form_shield:test_refusal_response_by_outcome]
def _with_query(path, query):
    return path + ("&" if "?" in path else "?") + query


# [@ANCHOR: form_shield:refusal_response]
# Verified by [@ANCHOR: form_shield:test_refusal_response_by_outcome]
def refusal_response(verdict, back_path, success_path, question_path=None):
    """The response for a refused Verdict.

    A program (SILENT) is sent to `success_path`, exactly where a real success goes. A visitor
    who sent no answer (QUESTION) is sent to `question_path` when the form has a page for the
    question, else back with the reason. A person who must retry is sent back to `back_path`
    with `form_shield_error=<reason>`, which error_notice.js turns into a notification. A rate
    limit or an unavailable store gets its own page with the right status code. All three paths
    must be local (start with "/"); callers pass literals or paths they built.
    """
    if verdict.outcome == SILENT:
        return request.redirect(success_path, local=True)
    if verdict.outcome in PAGE_STATUS:
        response = request.render(
            "form_shield.refused_page",
            {"verdict": verdict, "back_path": back_path},
        )
        response.status_code = PAGE_STATUS[verdict.outcome]
        response.headers["Cache-Control"] = "no-store"
        return response
    if verdict.outcome == QUESTION and question_path:
        return request.redirect(question_path, local=True)
    query = "form_shield_error=" + verdict.reason
    return request.redirect(_with_query(back_path, query), local=True)
