# -*- coding: utf-8 -*-
import logging

_logger = logging.getLogger(__name__)

def post_init_hook(env):
    """
    Hook executed upon module installation.
    Injects docs into the knowledge base using the centralized declarative facility.
    """
    if "ir.module.module" in env:
        # [@ANCHOR: soft_dependency_docs_installation]
        env["ir.module.module"]._bootstrap_knowledge_docs()
