use super::d3_timeline::D3Timeline;
use leptos::prelude::*;
use std::collections::VecDeque;
use stylance::import_style;

import_style!(style, "../../../styles/timeline.css");

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

    let websocket_events = use_context::<ReadSignal<VecDeque<serde_json::Value>>>()
        .unwrap_or_else(|| signal(VecDeque::new()).0);

    Effect::new(move |_| {
        let ws_events = websocket_events.get();
        let mut timeline_events = Vec::new();

        for (index, envelope) in ws_events.iter().enumerate() {
            if let Ok(timeline_event) = convert_envelope_to_timeline_event(envelope, index) {
                timeline_events.push(timeline_event);
            }
        }

        set_events.set(timeline_events);
    });

    Effect::new(move |_| {
        let mock_events = vec![
            TimelineEvent {
                id: "1".to_string(),
                timestamp: 0.0,
                event_type: "NodeStart".to_string(),
                description: "Node Alice started".to_string(),
                node_id: Some("alice".to_string()),
                metadata: None,
            },
            TimelineEvent {
                id: "2".to_string(),
                timestamp: 1.5,
                event_type: "KeyGen".to_string(),
                description: "Threshold key generation initiated".to_string(),
                node_id: None,
                metadata: None,
            },
            TimelineEvent {
                id: "3".to_string(),
                timestamp: 3.2,
                event_type: "NodeStart".to_string(),
                description: "Node Bob started".to_string(),
                node_id: Some("bob".to_string()),
                metadata: None,
            },
        ];
        set_events.set(mock_events);
    });

    view! {
        <div class=style::timeline_container>
            <div class=style::timeline_header>
                <h3>"Timeline"</h3>
                <div class=style::timeline_controls>
                    <button
                        class=style::control_button
                        on:click=move |_| set_is_playing.update(|p| *p = !*p)
                    >
                        {move || if is_playing.get() {
                            "Pause"
                        } else {
                            "Play"
                        }}
                    </button>

                    <select
                        class=style::speed_select
                        on:change=move |ev| {
                            let value = event_target_value(&ev).parse::<f64>().unwrap_or(1.0);
                            set_playback_speed.set(value);
                        }
                    >
                        <option value="0.5">"0.5x"</option>
                        <option value="1.0" selected>"1.0x"</option>
                        <option value="2.0">"2.0x"</option>
                        <option value="4.0">"4.0x"</option>
                    </select>
                </div>
            </div>

            <div class=style::timeline_visualization>
                <D3Timeline
                    events=events.into()
                    current_tick=current_time.into()
                    _zoom_level=signal(1.0).0.into()
                    _on_seek=Callback::new(move |tick: u64| {
                        log::info!("Seeking to tick: {}", tick);
                    })
                />
            </div>

            <div class=style::timeline_events>
                <For
                    each=move || events.get()
                    key=|event| event.id.clone()
                    children=move |event: TimelineEvent| {
                        view! {
                            <div class=style::timeline_event>
                                <div class=style::event_time>
                                    {format!("{:.1}s", event.timestamp)}
                                </div>
                                <div class=style::event_content>
                                    <div class=style::event_type>
                                        {event.event_type}
                                    </div>
                                    <div class=style::event_description>
                                        {event.description}
                                    </div>
                                    {event.node_id.map(|node_id| view! {
                                        <div class=style::event_node>
                                            {node_id}
                                        </div>
                                    })}
                                </div>
                            </div>
                        }
                    }
                />
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
