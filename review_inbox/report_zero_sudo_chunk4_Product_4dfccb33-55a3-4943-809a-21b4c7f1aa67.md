# Code Review Report: zero_sudo Chunk 4 (Product & UX)
**Reviewer:** Product_and_UX_Reviewer (Conversation: 4dfccb33-55a3-4943-809a-21b4c7f1aa67)

## 1. hams_open/zero_sudo/README.md
**Severity:** ERROR
**Finding:** ADR 0074 mandates that user-facing text nodes or layout headings in documentation manuals must utilize explicit inline HTML tracking attributes (e.g. `<h1 id="..." data-trace="...">`) rather than raw markdown text tags, and explicitly target their destination context namespace. Also, ADR MASTER 14 requires API contracts and `README.md` files to explicitly provide the exact, literal Python import path.
**File Path:** `hams_open/zero_sudo/README.md`

**TargetContent:**
```markdown
# Zero-Sudo Security Core [@ANCHOR: zero_sudo_main] (`zero_sudo`)
```
**ReplacementContent:**
```markdown
<h1 id="zero_sudo_main" data-trace="[@ANCHOR: zero_sudo:zero_sudo_main]">Zero-Sudo Security Core (<code>zero_sudo</code>)</h1>
```

**TargetContent:**
```markdown
## 3. Python API Reference (`zero_sudo.security.utils`)

#### `_get_service_uid(xml_id)` `[@ANCHOR: get_service_uid]`
```
**ReplacementContent:**
```markdown
## 3. Python API Reference (`zero_sudo.security.utils`)
**Import Path:** `hams_open/zero_sudo/models/security_utils.py`

<h4 id="get_service_uid" data-trace="[@ANCHOR: zero_sudo:get_service_uid]"><code>_get_service_uid(xml_id)</code></h4>
```

**TargetContent:**
```markdown
#### `_get_deterministic_hash(input_string)` `[@ANCHOR: deterministic_hash]`
```
**ReplacementContent:**
```markdown
<h4 id="deterministic_hash" data-trace="[@ANCHOR: zero_sudo:deterministic_hash]"><code>_get_deterministic_hash(input_string)</code></h4>
```

**TargetContent:**
```markdown
#### `_get_system_param(key, default=None)` `[@ANCHOR: get_system_param]`
```
**ReplacementContent:**
```markdown
<h4 id="get_system_param" data-trace="[@ANCHOR: zero_sudo:get_system_param]"><code>_get_system_param(key, default=None)</code></h4>
```

**TargetContent:**
```markdown
#### `_notify_cache_invalidation(model_name, key_value)` `[@ANCHOR: coherent_cache_signal]`
```
**ReplacementContent:**
```markdown
<h4 id="coherent_cache_signal" data-trace="[@ANCHOR: zero_sudo:coherent_cache_signal]"><code>_notify_cache_invalidation(model_name, key_value)</code></h4>
```

**TargetContent:**
```markdown
#### `_get_crypto_secret()` `[@ANCHOR: get_crypto_secret]`
```
**ReplacementContent:**
```markdown
<h4 id="get_crypto_secret" data-trace="[@ANCHOR: zero_sudo:get_crypto_secret]"><code>_get_crypto_secret()</code></h4>
```

**TargetContent:**
```markdown
#### `_invalidate_model_cache(model_name)` `[@ANCHOR: invalidate_model_cache]`
```
**ReplacementContent:**
```markdown
<h4 id="invalidate_model_cache" data-trace="[@ANCHOR: zero_sudo:invalidate_model_cache]"><code>_invalidate_model_cache(model_name)</code></h4>
```

**TargetContent:**
```markdown
#### `_set_kv(key, value)` `[@ANCHOR: set_kv_procedure]`
```
**ReplacementContent:**
```markdown
<h4 id="set_kv_procedure" data-trace="[@ANCHOR: zero_sudo:set_kv_procedure]"><code>_set_kv(key, value)</code></h4>
```

## 2. hams_open/zero_sudo/static/src/components/security_dashboard/security_dashboard.js
**Severity:** ERROR
**Finding:** ADR MASTER 13 requires that always-on dashboards must render in strict Dark Mode, and must implement a "continuous, pseudo-random CSS `transform: translate(x, y)` spatial drift on a slow (e.g., 20-second) linear transition". The component applies the transform every 20 seconds, but fails to apply the CSS `transition` property for the slow drift, resulting in jarring jumps instead of a continuous linear drift. It also does not enforce Dark Mode on the root element.
**File Path:** `hams_open/zero_sudo/static/src/components/security_dashboard/security_dashboard.js`
**Line Number:** 18

**TargetContent:**
```javascript
        useEffect(() => {
            const interval = setInterval(() => {
                if (this.dashboardRoot.el) {
```
**ReplacementContent:**
```javascript
        useEffect(() => {
            if (this.dashboardRoot.el) {
                this.dashboardRoot.el.classList.add("bg-dark", "text-white");
                this.dashboardRoot.el.style.transition = "transform 20s linear";
            }
            const interval = setInterval(() => {
                if (this.dashboardRoot.el) {
```

## 3. hams_open/zero_sudo/models/res_users.py
**Severity:** WARNING
**Finding:** Typographical error in the semantic anchor reference `COMM_COMM_test_service_account_password`.
**File Path:** `hams_open/zero_sudo/models/res_users.py`
**Line Number:** 42

**TargetContent:**
```python
        # # Verified by [@ANCHOR: zero_sudo:COMM_COMM_test_service_account_password]
```
**ReplacementContent:**
```python
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_service_account_password]
```

## 4. hams_open/zero_sudo/models/security_utils.py
**Severity:** WARNING
**Finding:** Typographical errors in semantic anchor references (`COMM_COMM_test_...`).
**File Path:** `hams_open/zero_sudo/models/security_utils.py`

**TargetContent:**
```python
        # # Verified by [@ANCHOR: zero_sudo:COMM_COMM_test_get_service_uid_sql_resolve]
```
**ReplacementContent:**
```python
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_get_service_uid_sql_resolve]
```

**TargetContent:**
```python
                    # # Verified by [@ANCHOR: zero_sudo:COMM_COMM_test_coherent_cache_signal_batch]
```
**ReplacementContent:**
```python
                    # # Verified by [@ANCHOR: zero_sudo:COMM_test_coherent_cache_signal_batch]
```

**TargetContent:**
```python
            # # Verified by [@ANCHOR: zero_sudo:COMM_COMM_test_coherent_cache_signal_single]
```
**ReplacementContent:**
```python
            # # Verified by [@ANCHOR: zero_sudo:COMM_test_coherent_cache_signal_single]
```

**TargetContent:**
```python
        # # Verified by [@ANCHOR: zero_sudo:COMM_COMM_test_set_kv_sql_check]
```
**ReplacementContent:**
```python
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_set_kv_sql_check]
```
