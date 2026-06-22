# Seamless Documentation Story

## Scenario
System Administrator Charlie installs the `user_websites_seo` module on a new Odoo instance. He expects the documentation for the module to be readily available within the system's Knowledge or Knowledge app.

## Story
During the installation of the module, or even if the Knowledge application is installed later, the `user_websites_seo` module automatically detects the presence of the `knowledge.article` model.

The module reads its internal documentation file and creates a new article titled "User Websites SEO Guide". It uses a dedicated maintenance service account to ensure the article is created with the correct permissions and categorized properly, even if Charlie is performing the installation from a restricted account.

Charlie can now find the "User Websites SEO Guide" in his Knowledge base, providing him and his users with immediate guidance on how to use the SEO features.

## Technical Anchors
- Dynamic Documentation Bootstrap: `[@ANCHOR: soft_dependency_docs_installation]`
