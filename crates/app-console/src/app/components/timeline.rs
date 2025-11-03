use super::d3_timeline::D3Timeline;
use crate::app::components::{ChevronDown, Pause, Play};
use crate::app::services::data_source::{use_data_source, DataSource};
use leptos::prelude::*;
use std::collections::VecDeque;

#[derive(Clone, Debug, serde::Serialize)]
pub struct TimelineEvent {
    pub id: String,
    pub timestamp: f64,
    pub event_type: String,
    pub description: String,
    pub node_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[component]
pub fn Timeline() -> impl IntoView {
    let (events, set_events) = signal(Vec::<TimelineEvent>::new());
    let (is_playing, set_is_playing) = signal(false);
    let (current_time, _set_current_time) = signal(0);
    let (_playback_speed, set_playback_speed) = signal(1.0);

    let data_source_manager = use_data_source();
    let current_source = data_source_manager.current_source();

    let websocket_events = use_context::<ReadSignal<VecDeque<serde_json::Value>>>()
        .unwrap_or_else(|| signal(VecDeque::new()).0);

    // Update events when data source changes
    Effect::new(move |_| {
        let source = current_source.get();
        log::info!("Timeline updating for data source: {:?}", source);

        match source {
            DataSource::Mock | DataSource::Simulator | DataSource::Real => {
                let service = data_source_manager.get_service();
                let timeline_events = service.get_timeline_events();
                set_events.set(timeline_events);
            }
        }
    });

    // WebSocket events effect (for real-time data when available)
    Effect::new(move |_| {
        if current_source.get() == DataSource::Real {
            let ws_events = websocket_events.get();
            let mut timeline_events = Vec::new();

            for (index, envelope) in ws_events.iter().enumerate() {
                if let Ok(timeline_event) = convert_envelope_to_timeline_event(envelope, index) {
                    timeline_events.push(timeline_event);
                }
            }

            if !timeline_events.is_empty() {
                set_events.set(timeline_events);
            }
        }
    });

    view! {
        <div class="flex flex-col h-full gap-4">
            <div class="flex-between gap-3">
                <h3 class="heading-1">"Timeline"</h3>
                <div class="flex gap-2 items-center">
                    <button
                        class="btn-icon"
                        on:click=move |_| set_is_playing.update(|p| *p = !*p)
                        title=move || if is_playing.get() { "Pause" } else { "Play" }
                    >
                        {move || if is_playing.get() {
                            view! { <Pause size=16 /> }.into_any()
                        } else {
                            view! { <Play size=16 /> }.into_any()
                        }}
                    </button>

                    <div class="flex items-center gap-1.5">
                        <div class="relative">
                            <select
                                class="input-base text-xs pl-2 pr-6 py-2 appearance-none cursor-pointer"
                                on:change=move |ev| {
                                    let value = event_target_value(&ev).parse::<f64>().unwrap_or(1.0);
                                    set_playback_speed.set(value);
                                }
                            >
                                <option value="0.5">"0.5×"</option>
                                <option value="1.0" selected>"1.0×"</option>
                                <option value="2.0">"2.0×"</option>
                                <option value="4.0">"4.0×"</option>
                            </select>
                            <div class="absolute right-1.5 top-1/2 -translate-y-1/2 pointer-events-none text-zinc-400 dark:text-zinc-500">
                                <ChevronDown size=14 />
                            </div>
                        </div>
                    </div>
                </div>
            </div>

            <div class="card-secondary p-4 h-40">
                <D3Timeline
                    events=events.into()
                    current_tick=current_time.into()
                    _zoom_level=signal(1.0).0.into()
                    _on_seek=Callback::new(move |tick: u64| {
                        log::info!("Seeking to tick: {}", tick);
                    })
                />
            </div>

            <div class="flex-1 overflow-y-auto">
                <div class="space-y-2">
                    <For
                        each=move || events.get()
                        key=|event| event.id.clone()
                        children=move |event: TimelineEvent| {
                            view! {
                                <div class="card-secondary card-compact flex gap-3 border border-transparent hover:border-[#bc6de3]">
                                    <div class="flex-shrink-0 w-14 text-right">
                                        <span class="text-xs text-zinc-500 dark:text-zinc-400 font-mono">{format!("{:.1}s", event.timestamp)}</span>
                                    </div>
                                    <div class="flex-1 min-w-0">
                                        <div class="text-sm font-semibold text-zinc-900 dark:text-zinc-50">{event.event_type}</div>
                                        <p class="text-xs text-zinc-600 dark:text-zinc-400 mt-0.5">{event.description}</p>
                                        {event.node_id.map(|node_id| view! {
                                            <div class="text-xs highlight font-medium mt-1">"Node: "{node_id}</div>
                                        })}
                                    </div>
                                </div>
                            }
                        }
                    />
                </div>
            </div>
        </div>
    }
}

fn convert_envelope_to_timeline_event(
    envelope: &serde_json::Value,
    index: usize,
) -> Result<TimelineEvent, String> {
    let id = index.to_string();
    let timestamp = envelope
        .get("timestamp")
        .and_then(|t| t.as_f64())
        .unwrap_or(index as f64);

    let event_type = envelope
        .get("message_type")
        .and_then(|mt| mt.as_str())
        .unwrap_or("unknown")
        .to_string();

    let description = envelope
        .get("payload")
        .and_then(|p| p.get("description"))
        .and_then(|d| d.as_str())
        .unwrap_or(&format!("{} event occurred", event_type))
        .to_string();

    let node_id = envelope
        .get("payload")
        .and_then(|p| p.get("node_id"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());

    let metadata = envelope.get("payload").cloned();

    Ok(TimelineEvent {
        id,
        timestamp,
        event_type,
        description,
        node_id,
        metadata,
    })
}
