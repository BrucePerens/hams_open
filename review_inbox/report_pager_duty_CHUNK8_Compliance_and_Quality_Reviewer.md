# Compliance and Quality Review Report
**Module:** `hams_open/pager_duty`
**Chunk:** 8
**Reviewer Role:** Compliance_and_Quality_Reviewer

## 1. `hams_open/pager_duty/views/board_templates.xml`
- **Severity:** ERROR
- **Issue:** Missing `<!-- burn-ignore-tour -->` or JS UI Tour anchor (ADR 0076). Assuming this dashboard doesn't have complex states, it needs the bypass tag alongside the view test bypass.
- **Location:** Line 4
- **Original Code Snippet**:
```xml
    <template id="board_template" name="Pager Duty Board">
        <!-- audit-ignore-view: Tested by [@ANCHOR: test_pager_board_url] -->
        <html>
```
- **TargetContent**:
```xml
    <template id="board_template" name="Pager Duty Board">
        <!-- audit-ignore-view: Tested by [@ANCHOR: test_pager_board_url] -->
        <html>
```
- **ReplacementContent**:
```xml
    <template id="board_template" name="Pager Duty Board">
        <!-- audit-ignore-view: Tested by [@ANCHOR: test_pager_board_url] -->
        <!-- burn-ignore-tour -->
        <html>
```

## 2. `hams_open/pager_duty/views/incident_views.xml`
- **Severity:** ERROR
- **Issue:** Missing `<!-- burn-ignore-tour -->` for simple views (ADR 0076).
- **Location:** Lines 3-5, 20-22
- **Original Code Snippet 1**:
```xml
    <record id="view_pager_incident_list" model="ir.ui.view">
        <!-- audit-ignore-view: Tested by [@ANCHOR: test_pager_view] -->
        <field name="name">pager.incident.list</field>
```
- **TargetContent 1**:
```xml
    <record id="view_pager_incident_list" model="ir.ui.view">
        <!-- audit-ignore-view: Tested by [@ANCHOR: test_pager_view] -->
        <field name="name">pager.incident.list</field>
```
- **ReplacementContent 1**:
```xml
    <record id="view_pager_incident_list" model="ir.ui.view">
        <!-- audit-ignore-view: Tested by [@ANCHOR: test_pager_view] -->
        <!-- burn-ignore-tour -->
        <field name="name">pager.incident.list</field>
```
- **Original Code Snippet 2**:
```xml
    <record id="view_pager_incident_kanban" model="ir.ui.view">
        <!-- audit-ignore-view: Tested by [@ANCHOR: test_pager_view] -->
        <field name="name">pager.incident.kanban</field>
```
- **TargetContent 2**:
```xml
    <record id="view_pager_incident_kanban" model="ir.ui.view">
        <!-- audit-ignore-view: Tested by [@ANCHOR: test_pager_view] -->
        <field name="name">pager.incident.kanban</field>
```
- **ReplacementContent 2**:
```xml
    <record id="view_pager_incident_kanban" model="ir.ui.view">
        <!-- audit-ignore-view: Tested by [@ANCHOR: test_pager_view] -->
        <!-- burn-ignore-tour -->
        <field name="name">pager.incident.kanban</field>
```

## 3. `hams_open/pager_duty/views/log_analyzer_views.xml`
- **Severity:** CRITICAL
- **Issue:** Placement of `audit-ignore-view` is invalid. It must be inside the `<record>` element as a direct child node. Also, `<!-- Verified by -->` is an incorrect linter tag; it should be `<!-- audit-ignore-view: Tested by ... -->`. Finally, missing `burn-ignore-tour` for simple lookup tables.
- **Location:** Lines 3-6, 18-21
- **Original Code Snippet 1**:
```xml
    <!-- Verified by [@ANCHOR: test_log_analyzer_views] -->
    <record id="view_pager_log_pattern_list" model="ir.ui.view">
        <field name="name">pager.log.pattern.list</field>
```
- **TargetContent 1**:
```xml
    <!-- Verified by [@ANCHOR: test_log_analyzer_views] -->
    <record id="view_pager_log_pattern_list" model="ir.ui.view">
        <field name="name">pager.log.pattern.list</field>
```
- **ReplacementContent 1**:
```xml
    <record id="view_pager_log_pattern_list" model="ir.ui.view">
        <!-- audit-ignore-view: Tested by [@ANCHOR: test_log_analyzer_views] -->
        <!-- burn-ignore-tour -->
        <field name="name">pager.log.pattern.list</field>
```
- **Original Code Snippet 2**:
```xml
    <!-- Verified by [@ANCHOR: test_log_analyzer_views] -->
    <record id="view_pager_log_file_list" model="ir.ui.view">
        <field name="name">pager.log.file.list</field>
```
- **TargetContent 2**:
```xml
    <!-- Verified by [@ANCHOR: test_log_analyzer_views] -->
    <record id="view_pager_log_file_list" model="ir.ui.view">
        <field name="name">pager.log.file.list</field>
```
- **ReplacementContent 2**:
```xml
    <record id="view_pager_log_file_list" model="ir.ui.view">
        <!-- audit-ignore-view: Tested by [@ANCHOR: test_log_analyzer_views] -->
        <!-- burn-ignore-tour -->
        <field name="name">pager.log.file.list</field>
```

## 4. `hams_open/pager_duty/views/pager_check_views.xml`
- **Severity:** CRITICAL
- **Issue:** Placement of `audit-ignore-view` is invalid (must be a direct child node inside `<record>`). Also, `view_pager_check_form` contains complex dynamic visibility (e.g. `invisible="check_type not in ('http', 'http3', 'snmp')"`) which MANDATES a JS UI Tour under ADR-0076. No `burn-ignore-tour` is permitted for the form view. A UI Tour must be written.
- **Location:** Lines 3-6, 22-26
- **Original Code Snippet 1**:
```xml
    <!-- audit-ignore-view: Tested by [@ANCHOR: test_pager_view] -->
    <record id="view_pager_check_list" model="ir.ui.view">
        <field name="name">pager.check.list</field>
```
- **TargetContent 1**:
```xml
    <!-- audit-ignore-view: Tested by [@ANCHOR: test_pager_view] -->
    <record id="view_pager_check_list" model="ir.ui.view">
        <field name="name">pager.check.list</field>
```
- **ReplacementContent 1**:
```xml
    <record id="view_pager_check_list" model="ir.ui.view">
        <!-- audit-ignore-view: Tested by [@ANCHOR: test_pager_view] -->
        <!-- burn-ignore-tour -->
        <field name="name">pager.check.list</field>
```
- **Original Code Snippet 2**:
```xml
    <!-- audit-ignore-view: Tested by [@ANCHOR: test_pager_view] -->

    <record id="view_pager_check_form" model="ir.ui.view">
        <field name="name">pager.check.form</field>
```
- **TargetContent 2**:
```xml
    <!-- audit-ignore-view: Tested by [@ANCHOR: test_pager_view] -->

    <record id="view_pager_check_form" model="ir.ui.view">
        <field name="name">pager.check.form</field>
```
- **ReplacementContent 2**:
```xml
    <record id="view_pager_check_form" model="ir.ui.view">
        <!-- audit-ignore-view: Tested by [@ANCHOR: test_pager_view] -->
        <!-- TODO: A JS UI Tour MUST be created for this form due to dynamic invisible state logic (ADR 0076). Once created, add the tour link anchor here. -->
        <field name="name">pager.check.form</field>
```

## 5. `hams_open/pager_duty/views/schedule_views.xml`
- **Severity:** CRITICAL
- **Issue:** Placement of `audit-ignore-view` is invalid (must be a direct child node inside `<record>`). Also missing `burn-ignore-tour` for a micro-inheritance view.
- **Location:** Lines 3-6
- **Original Code Snippet**:
```xml
    <!-- audit-ignore-view: Tested by [@ANCHOR: test_pager_view] -->
    <record id="view_calendar_event_form_inherit_pager" model="ir.ui.view">
        <field name="name">calendar.event.form.inherit.pager</field>
```
- **TargetContent**:
```xml
    <!-- audit-ignore-view: Tested by [@ANCHOR: test_pager_view] -->
    <record id="view_calendar_event_form_inherit_pager" model="ir.ui.view">
        <field name="name">calendar.event.form.inherit.pager</field>
```
- **ReplacementContent**:
```xml
    <record id="view_calendar_event_form_inherit_pager" model="ir.ui.view">
        <!-- audit-ignore-view: Tested by [@ANCHOR: test_pager_view] -->
        <!-- burn-ignore-tour -->
        <field name="name">calendar.event.form.inherit.pager</field>
```

## 6. `hams_open/pager_duty/wizard/__init__.py`
- **Severity:** ERROR
- **Issue:** Missing correct `SPDX-License-Identifier: AGPL-3.0-or-later` header.
- **Location:** Lines 1-4
- **Original Code Snippet**:
```python
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
```
- **TargetContent**:
```python
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
```
- **ReplacementContent**:
```python
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
```
