import os

def post_init_hook(env):
    # [@ANCHOR: helpdesk_doc_injection]
    doc_path = os.path.join(os.path.dirname(__file__), "data", "documentation.html")
    if os.path.exists(doc_path):
        with open(doc_path, "r", encoding="utf-8") as f:
            content = f.read()

        # Inject into the central manual library / documentation parameter
        env["ir.config_parameter"].set_param("hams_helpdesk.documentation", content)
