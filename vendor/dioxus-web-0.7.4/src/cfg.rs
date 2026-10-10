use std::rc::Rc;

use dioxus_core::LaunchConfig;
use wasm_bindgen::JsCast as _;

/// Synchronous observation of one actual renderer edit batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderBatchPhase {
    /// Before preparing this batch: previous layout observations may be revoked.
    BeforeRender,
    /// After VirtualDom render, immediately before applying the complete edits.
    BeforeCommit,
    /// After applying edits and dispatching actual registered-node mounted events.
    Committed,
    /// Hydration does not provide this edit-batch observation contract.
    HydrationUnavailable,
    /// External asset changes do not provide a synchronous layout commit contract.
    ExternalMutationUnavailable,
}

/// Configuration for the WebSys renderer for the Dioxus VirtualDOM.
///
/// This struct configures hydration and render destination.
///
/// # Example
///
/// ```rust, ignore
/// dioxus_web::launch(App, Config::new().hydrate(true).root_name("myroot"))
/// ```
pub struct Config {
    pub(crate) hydrate: bool,
    pub(crate) render_batch_observer: Option<Rc<dyn Fn(RenderBatchPhase)>>,
    #[allow(dead_code)]
    pub(crate) panic_hook: bool,
    pub(crate) root: ConfigRoot,
    #[cfg(feature = "document")]
    pub(crate) history: Option<Rc<dyn dioxus_history::History>>,
}

impl LaunchConfig for Config {}

pub(crate) enum ConfigRoot {
    RootName(String),
    RootNode(web_sys::Node),
}

impl Config {
    /// Create a new Default instance of the Config.
    ///
    /// This is no different than calling `Config::default()`
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(feature = "hydrate")]
    /// Enable SSR hydration
    ///
    /// This enables Dioxus to pick up work from a pre-rendered HTML file. Hydration will completely skip over any async
    /// work and suspended nodes.
    ///
    /// Dioxus will load up all the elements with the `dio_el` data attribute into memory when the page is loaded.
    pub fn hydrate(mut self, f: bool) -> Self {
        self.hydrate = f;
        self
    }

    /// Set the name of the element that Dioxus will use as the root.
    ///
    /// This is akin to calling React.render() on the element with the specified name.
    /// Note that this only works on the current document, i.e. `window.document`.
    /// To use a different document (popup, iframe, ...) use [Self::rootelement] instead.
    pub fn rootname(mut self, name: impl Into<String>) -> Self {
        self.root = ConfigRoot::RootName(name.into());
        self
    }

    /// Set the element that Dioxus will use as root.
    ///
    /// This is akin to calling React.render() on the given element.
    pub fn rootelement(mut self, elem: web_sys::Element) -> Self {
        self.root = ConfigRoot::RootNode(elem.unchecked_into());
        self
    }

    /// Set the node that Dioxus will use as root.
    ///
    /// This is akin to calling React.render() on the given element.
    pub fn rootnode(mut self, node: web_sys::Node) -> Self {
        self.root = ConfigRoot::RootNode(node);
        self
    }

    /// Observe actual synchronous edit-batch boundaries.
    /// This callback must observe only; it does not acknowledge downstream
    /// subscriber processing, browser painting, or protocol completion.
    pub fn render_batch_observer(mut self, observer: Rc<dyn Fn(RenderBatchPhase)>) -> Self {
        self.render_batch_observer = Some(observer);
        self
    }

    /// Set the history provider for the application.
    ///
    /// `dioxus-web` provides two history providers:
    /// - `dioxus_history::WebHistory`: A history provider that uses the browser history API.
    /// - `dioxus_history::HashHistory`: A history provider that uses the `#` url fragment.
    ///
    /// By default, `dioxus-web` uses the `WebHistory` provider, but this method can be used to configure
    /// a different history provider.
    pub fn history(mut self, history: Rc<dyn dioxus_history::History>) -> Self {
        #[cfg(feature = "document")]
        {
            self.history = Some(history);
        }
        self
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hydrate: false,
            render_batch_observer: None,
            root: ConfigRoot::RootName("main".to_string()),
            #[cfg(feature = "document")]
            history: None,
            panic_hook: true,
        }
    }
}
