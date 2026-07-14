# Journey: Extension and Customization

This journey describes how the platform can be extended or customized by other modules or administrators using standardized "dropzones".

## Path: UI Customization via Dropzones

The module provides several stable XPath anchors (dropzones) in its QWeb templates to allow safe extension of the user interface without breaking core functionality.

1. **Navigation**: Extensions can inject items into the global navbar ([@ANCHOR: dropzone_navbar]).

2. **Layout**: Custom CSS or JS can be injected into the main website layout ([@ANCHOR: dropzone_layout]).

3. **Snippets**: New drag-and-drop building blocks can be added to the snippets sidebar ([@ANCHOR: dropzone_snippets]).

4. **User Settings**: Additional configuration options can be added to the user settings portal ([@ANCHOR: dropzone_users]).

## Path: Architectural Extension

1. **Rendering**: Custom logic can be hooked into the rendering process of various components like the blog post ([@ANCHOR: xpath_rendering_blog_post]) or the navbar ([@ANCHOR: xpath_rendering_navbar]).

2. **Templates**: New portal templates can be registered via the templates dropzone ([@ANCHOR: dropzone_templates]).

## Technical Notes
- These dropzones are verified by automated rendering tests to ensure they remain stable ([@ANCHOR: test_dropzone_snippets], [@ANCHOR: test_dropzone_layout]).

- The system uses a specific XPath rendering pattern for high-performance UI assembly ([@ANCHOR: xpath_rendering_layout]).
