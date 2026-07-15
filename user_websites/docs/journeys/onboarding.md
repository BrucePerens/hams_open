# Journey: First Time Site Setup

This journey describes the path a new user takes to establish their presence on the platform.

## Path: Onboarding and Creation

1. **Discovery**: The user navigates to the Community Directory ([ANCHOR: UX_COMMUNITY_DIRECTORY]) to see what others have built. Verified by `[ANCHOR: test_tour_community_directory]`.
2. **Accessing Profile**: The user clicks on their own name in the navbar or enters their slug directly (e.g., `/alice`).
3. **Routing**: The system resolves the slug and determines no site exists yet ([ANCHOR: controller_user_websites_home]). Verified by `[ANCHOR: test_tour_create_site]`.
4. **Placeholder**: The user sees the placeholder page with the "Create My Site" call-to-action.
5. **Initialization**: The user clicks the button, triggering the `create_site` controller ([ANCHOR: UX_CREATE_SITE]). Verified by `[ANCHOR: test_tour_create_site]`.

6. **Security Check**: The system verifies the user owns the slug and hasn't exceeded their page quota ([ANCHOR: website_page_quota_check]). Verified by `[ANCHOR: test_page_quota_limit]`.

7. **Provisioning**: The system creates the `website.page` record using a proxy service account ([ANCHOR: mixin_proxy_ownership_create]). Verified by `[ANCHOR: test_mixin_ownership_validation]`.
8. **Redirect**: The user is redirected to their live home page, now ready for customization.

## Path: Starting a Blog

1. **Blog Index**: The user navigates to `/alice/blog` ([ANCHOR: controller_user_blog_index]). Verified by `[ANCHOR: test_tour_create_blog]`.

2. **First Post**: The user clicks "Create Blog Post" ([ANCHOR: UX_CREATE_BLOG_POST]). Verified by `[ANCHOR: test_tour_create_blog]`.
3. **Provisioning**: The system initializes the `blog.blog` container for the user if it doesn't exist and creates a "Welcome" post.
4. **Engagement**: Visitors can now subscribe to Alice's updates ([ANCHOR: UX_SUBSCRIBE]). Verified by `[ANCHOR: test_subscription_creation]`.

## Path: Identity Verification

1. **QRZ Token Generation**: The system provisions a QRZ linkage token for the user ([ANCHOR: ham_onboarding:action_generate_qrz_token]).

2. **OTP Dispatch**: The system sends an email with an official OTP pass-code using the OTP mail template ([ANCHOR: ham_onboarding:otp_mail_template]).

3. **Official OTP Verification**: The system verifies the identity using an official OTP pass-code ([ANCHOR: ham_onboarding:action_verify_official_otp]).
