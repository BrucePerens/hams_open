# JULES ISSUES - backup_management

## Connection Testing
The "Test Connection" feature for local directories performs a basic existence check on the Odoo web node. However, since backup operations are offloaded to an asynchronous worker (RabbitMQ Bastion), the actual accessibility depends on the worker's filesystem mounts. A warning is logged to the chatter if the path is missing on the web node, but the test is still queued for the worker to provide definitive feedback.
