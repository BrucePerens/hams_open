# Code Review Report: User Websites Chunk 2
**Role:** Product and UX Reviewer
**Module:** user_websites
**Date:** 2026-07-15
**Reviewer ID:** fc5dc99d-f010-4d82-a13b-b47f2ecd553a

## Overview
A comprehensive architectural, UX, and security review has been completed for the `user_websites` module (Chunk 2, covering `controllers/` and `security/`). The integration with the `zero_sudo` service pattern is generally well-implemented, enforcing privilege separation for public-facing endpoints. However, critical gaps were identified regarding exception handling for the Redis cache and bypasses of the suspension mechanism in creation endpoints.

## Findings

### 1. High Priority - Redis Unavailability Causes 500 Errors on Home Fallback Route
- **File:** `hams_open/user_websites/controllers/main.py`
- **Line:** 221
- **Snippet:**
  ```python
            try:
                if not request.env.user._is_admin():
                    db_name = request.env.cr.dbname
                    redis_client.incr(f"views:{db_name}:page:{page.id}")
            except (KeyError, ValueError) as e:   # Tested by [@ANCHOR: test_cron_redis_flush]
  ```
- **Description:** The `user_home_fallback` route increments a Redis view counter. However, `redis_client.incr()` can raise `redis.exceptions.RedisError` (e.g., `ConnectionError`, `TimeoutError`) if the Redis service is temporarily down. Catching only `KeyError` and `ValueError` means any Redis connectivity failure will result in an uncaught exception, crashing the entire route and rendering the user's home page inaccessible. This violates the architectural mandate for resilience (ADR MASTER_04).
- **Actionable Fix:** Import `redis` exceptions and expand the `except` block to catch `redis.exceptions.RedisError` (or a broader `Exception` if acceptable) to ensure the route degrades gracefully when the cache is down.

### 2. High Priority - Suspended Users Can Bypass Bans to Create Sites and Blogs
- **File:** `hams_open/user_websites/controllers/main.py`
- **Line:** 272-279 (`create_site`), 334-337 (`create_blog`)
- **Snippet (create_site):**
  ```python
        if profile_user and profile_user.id != user.id:
            raise request.not_found()
        if profile_group:
            is_member = env_svc["user.websites.group"].search_count(
                [("id", "=", profile_group.id), ("member_ids", "=", user.id)]
            )
            if not is_member:
                raise request.not_found()
  ```
- **Description:** The `/create_site` and `/create_blog` POST endpoints utilize the `env_svc` (Zero-Sudo service account environment) to create `website.page` and `blog.blog` records. This pattern bypasses standard Odoo `ir.rule` evaluations. While the routes properly verify ownership and group membership, they **fail to verify the suspension status** of the user or group (`is_suspended_from_websites`). A suspended user or group can exploit these routes to generate new sites and content, completely bypassing their suspension.
- **Actionable Fix:** Add explicit suspension checks directly before or after the ownership checks in both `create_site` and `create_blog`.
  ```python
        if profile_user and profile_user.is_suspended_from_websites:
            raise request.not_found()
        if profile_group and profile_group.is_suspended_from_websites:
            raise request.not_found()
  ```

### 3. Medium Priority - Missing Safe Access/Validation in `user_websites_api.py` (Resilience)
- **File:** `hams_open/user_websites/controllers/user_websites_api.py`
- **Line:** 46
- **Snippet:**
  ```python
            except (KeyError, ValueError) as e:   # Tested by [@ANCHOR: test_domains_api_returns_all_domains]
                _logger.warning("Failed to fetch ham.dns.zone domains: %s", e)
  ```
- **Description:** Similar to the Redis issue, querying `ham.dns.zone` might fail due to database errors or RPC issues that raise exceptions other than `KeyError` or `ValueError` (e.g., `psycopg2.OperationalError` or generic `Exception`). If this API fails, the Let's Encrypt renewal pipeline could be interrupted. 
- **Actionable Fix:** Broaden the exception catch block to `except Exception as e:` to guarantee that `api_domains` always falls back cleanly to returning the `edge_domains` array, avoiding a crash that breaks domain maintenance.

## Final Decision
**Status:** Requires Changes.
The module demonstrates excellent adherence to the Zero-Sudo architecture and GDPR guidelines. However, the identified security bypass (Suspended users able to create content via service account) and resilience gap (Redis exceptions causing 500 errors) must be patched prior to deployment.
