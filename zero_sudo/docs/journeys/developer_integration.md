<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Journey: Developer Integration `[@ANCHOR: zero_sudo:COMM_journey_developer_integration]`

This journey describes how a developer integrates their module with the `zero_sudo` core.

## 1. Dependency Declaration
The developer adds `zero_sudo` to the `depends` list in their `__manifest__.py`.

## 2. Defining a Service Account
The developer creates a service account in their module's XML data, ensuring `is_service_account` is set to `True`.

## 3. Using Security Utilities
The developer replaces any direct `.sudo()` calls with `self.env['zero_sudo.security.utils']._get_service_uid('my_module.my_service_account')`.

## 4. Registering System Parameters
If the module needs to read/write system parameters via `zero_sudo`, the developer adds them to the whitelist by creating an `_inherit` extension of `zero_sudo.security.utils`.

## 5. Adding Documentation
The developer adds a `knowledge_docs` entry to their `__manifest__.py` to automatically install their module's documentation.
