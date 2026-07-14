import re

def fix_file(filepath):
    with open(filepath, 'r') as f:
        content = f.read()

    # Find all elements with data-trace="[@ANCHOR: ...]"
    # Extract the anchor text and insert an HTML comment above it
    def replacer(match):
        anchor_text = match.group(1)
        # return the comment on a new line, and the element without data-trace
        return f"<!-- [@ANCHOR: {anchor_text}] -->\n{match.group(2)}"

    # We match <h1 ... data-trace="[@ANCHOR: ...]">...
    # Actually, let's just match data-trace="\[@ANCHOR: (.*?)\]" inside a tag
    # It's safer to do this carefully.
    
    # 1. Replace data-trace="[@ANCHOR: xxx]" with nothing, but remember xxx to put above.
    # regex: (<h[1-6].*?) data-trace="\[@ANCHOR: (.*?)\]"(.*?>)
    content = re.sub(r'(<h[1-6].*?) data-trace="\[@ANCHOR: (.*?)\]"(.*?>)', r'<!-- [@ANCHOR: \2] -->\n\1\3', content)

    # 2. Fix the spans that we added earlier
    content = re.sub(r'<span style="display:none;">\[@ANCHOR: (.*?)\]</span>', r'<!-- [@ANCHOR: \1] -->', content)

    with open(filepath, 'w') as f:
        f.write(content)

fix_file('zero_sudo/data/documentation.html')
fix_file('zero_sudo/data/testing_documentation.html')

