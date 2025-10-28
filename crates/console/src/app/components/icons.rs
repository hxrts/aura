use leptos::prelude::*;

/// Sun icon from Lucide
#[component]
pub fn Sun(
    #[prop(optional)] size: Option<u32>,
    #[prop(optional)] color: Option<&'static str>,
) -> impl IntoView {
    let size = size.unwrap_or(24);
    let color = color.unwrap_or("currentColor");

    view! {
        <svg
            xmlns="http://www.w3.org/2000/svg"
            width=size
            height=size
            viewBox="0 0 24 24"
            fill="none"
            stroke=color
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
        >
            <circle cx="12" cy="12" r="4"></circle>
            <path d="M12 2v2"></path>
            <path d="M12 20v2"></path>
            <path d="m4.93 4.93 1.41 1.41"></path>
            <path d="m17.66 17.66 1.41 1.41"></path>
            <path d="M2 12h2"></path>
            <path d="M20 12h2"></path>
            <path d="m6.34 17.66-1.41 1.41"></path>
            <path d="m19.07 4.93-1.41 1.41"></path>
        </svg>
    }
}

/// Moon icon from Lucide
#[component]
pub fn Moon(
    #[prop(optional)] size: Option<u32>,
    #[prop(optional)] color: Option<&'static str>,
) -> impl IntoView {
    let size = size.unwrap_or(24);
    let color = color.unwrap_or("currentColor");

    view! {
        <svg
            xmlns="http://www.w3.org/2000/svg"
            width=size
            height=size
            viewBox="0 0 24 24"
            fill="none"
            stroke=color
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
        >
            <path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"></path>
        </svg>
    }
}

/// GitFork icon from Lucide
#[component]
pub fn GitFork(
    #[prop(optional)] size: Option<u32>,
    #[prop(optional)] color: Option<&'static str>,
) -> impl IntoView {
    let size = size.unwrap_or(24);
    let color = color.unwrap_or("currentColor");

    view! {
        <svg
            xmlns="http://www.w3.org/2000/svg"
            width=size
            height=size
            viewBox="0 0 24 24"
            fill="none"
            stroke=color
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
        >
            <circle cx="12" cy="18" r="2"></circle>
            <circle cx="7" cy="6" r="2"></circle>
            <circle cx="17" cy="6" r="2"></circle>
            <path d="M7 8v2a2 2 0 0 0 2 2h6a2 2 0 0 0 2 -2V8"></path>
            <line x1="12" y1="12" x2="12" y2="16"></line>
        </svg>
    }
}

/// ChevronDown icon from Lucide
#[component]
pub fn ChevronDown(
    #[prop(optional)] size: Option<u32>,
    #[prop(optional)] color: Option<&'static str>,
) -> impl IntoView {
    let size = size.unwrap_or(24);
    let color = color.unwrap_or("currentColor");

    view! {
        <svg
            xmlns="http://www.w3.org/2000/svg"
            width=size
            height=size
            viewBox="0 0 24 24"
            fill="none"
            stroke=color
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
        >
            <path d="m6 9 6 6 6-6"></path>
        </svg>
    }
}

/// ChevronRight icon from Lucide
#[component]
pub fn ChevronRight(
    #[prop(optional)] size: Option<u32>,
    #[prop(optional)] color: Option<&'static str>,
) -> impl IntoView {
    let size = size.unwrap_or(24);
    let color = color.unwrap_or("currentColor");

    view! {
        <svg
            xmlns="http://www.w3.org/2000/svg"
            width=size
            height=size
            viewBox="0 0 24 24"
            fill="none"
            stroke=color
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
        >
            <path d="m9 18 6-6-6-6"></path>
        </svg>
    }
}

/// Play icon from Lucide
#[component]
pub fn Play(
    #[prop(optional)] size: Option<u32>,
    #[prop(optional)] color: Option<&'static str>,
) -> impl IntoView {
    let size = size.unwrap_or(24);
    let color = color.unwrap_or("currentColor");

    view! {
        <svg
            xmlns="http://www.w3.org/2000/svg"
            width=size
            height=size
            viewBox="0 0 24 24"
            fill="none"
            stroke=color
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
        >
            <polygon points="5 3 19 12 5 21 5 3"></polygon>
        </svg>
    }
}

/// Pause icon from Lucide
#[component]
pub fn Pause(
    #[prop(optional)] size: Option<u32>,
    #[prop(optional)] color: Option<&'static str>,
) -> impl IntoView {
    let size = size.unwrap_or(24);
    let color = color.unwrap_or("currentColor");

    view! {
        <svg
            xmlns="http://www.w3.org/2000/svg"
            width=size
            height=size
            viewBox="0 0 24 24"
            fill="none"
            stroke=color
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
        >
            <rect x="6" y="4" width="4" height="16"></rect>
            <rect x="14" y="4" width="4" height="16"></rect>
        </svg>
    }
}
