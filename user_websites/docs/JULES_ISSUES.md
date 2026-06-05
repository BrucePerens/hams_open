# JULES ISSUES - user_websites

## Environment Hurdles
- RabbitMQ was not starting initially due to incorrect permissions on `/var/lib/rabbitmq/.erlang.cookie`. Fixed by setting permissions to 400 and restarting the service.

## Framework Bugs / Hurdles
- Odoo 19 Owl UI tours are prone to race conditions. Using `TourUtils` from `zero_sudo` is recommended.

## Missing Resources
- None identified yet.
