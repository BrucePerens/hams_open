import re

with open("/home/bruce/workspace/hams_open/cloudflare/utils/cloudflare_api.py", "r") as f:
    content = f.read()

# Add _handle_api_error helper
helper = """
def _handle_api_error(context_msg, exception):
    if "External requests verboten" in str(exception):
        _logger.info("%s (Disabled in tests): %s", context_msg, exception)
    else:
        _logger.error("%s: %s", context_msg, exception)

"""

# Insert after _logger = logging.getLogger(__name__)
content = content.replace("session = requests.Session()", helper + "session = requests.Session()")

# Replace all _logger.error("...: %s", e)
content = re.sub(
    r'_logger\.error\("([^"]+):\s*%s",\s*e\)',
    r'_handle_api_error("\1", e)',
    content
)

with open("/home/bruce/workspace/hams_open/cloudflare/utils/cloudflare_api.py", "w") as f:
    f.write(content)
