use super::timeline::TimelineEvent;
use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use web_sys::HtmlElement;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = d3)]
    fn select(selector: &str) -> JsValue;

    #[wasm_bindgen(js_namespace = d3)]
    fn scaleLinear() -> JsValue;

    #[wasm_bindgen(js_namespace = d3)]
    fn scaleTime() -> JsValue;

    #[wasm_bindgen(js_namespace = d3)]
    fn axisBottom(scale: &JsValue) -> JsValue;

    #[wasm_bindgen(js_namespace = d3)]
    fn zoom() -> JsValue;
}

#[component]
pub fn D3Timeline(
    events: Signal<Vec<TimelineEvent>>,
    current_tick: Signal<u64>,
    _zoom_level: Signal<f64>,
    _on_seek: Callback<u64>,
) -> impl IntoView {
    let timeline_ref = NodeRef::<leptos::html::Div>::new();
    let (svg_created, set_svg_created) = signal(false);

    // Initialize D3 timeline when component mounts
    Effect::new(move |_| {
        if let Some(container) = timeline_ref.get() {
            if !svg_created.get() {
                init_d3_timeline(&container);
                set_svg_created.set(true);
            }
        }
    });

    // Update timeline when events change
    Effect::new(move |_| {
        let events_data = events.get();
        if svg_created.get() && !events_data.is_empty() {
            update_timeline_data(&events_data, current_tick.get());
        }
    });

    // Update timeline position when current tick changes
    Effect::new(move |_| {
        let tick = current_tick.get();
        if svg_created.get() {
            update_current_position(tick);
        }
    });

    view! {
        <div
            node_ref=timeline_ref
            class="d3-timeline-container"
            style="width: 100%; height: 180px; position: relative;"
        >
            // D3 SVG will be created here
        </div>
    }
}

fn init_d3_timeline(container: &HtmlElement) {
    let js_code = format!(
        r#"
        const container = arguments[0];
        const width = container.clientWidth;
        const height = 180;
        const margin = {{top: 20, right: 30, bottom: 40, left: 40}};

        // Clear any existing content
        d3.select(container).selectAll("*").remove();

        // Create SVG
        const svg = d3.select(container)
            .append("svg")
            .attr("width", width)
            .attr("height", height)
            .style("background", "var(--bg-secondary)");

        // Create main group
        const g = svg.append("g")
            .attr("transform", `translate(${{margin.left}},${{margin.top}})`);

        // Store references for later updates
        container._d3Timeline = {{
            svg: svg,
            g: g,
            width: width - margin.left - margin.right,
            height: height - margin.top - margin.bottom,
            margin: margin,
            xScale: d3.scaleLinear().range([0, width - margin.left - margin.right]),
            yScale: d3.scaleLinear().range([height - margin.top - margin.bottom, 0])
        }};

        // Add X axis group
        g.append("g")
            .attr("class", "x-axis")
            .attr("transform", `translate(0,${{container._d3Timeline.height}})`);

        // Add current position indicator
        g.append("line")
            .attr("class", "current-position")
            .attr("y1", 0)
            .attr("y2", container._d3Timeline.height)
            .attr("stroke", "var(--color-primary)")
            .attr("stroke-width", 2)
            .style("opacity", 0);

        // Add click handler for seeking
        svg.on("click", function(event) {{
            const [x, y] = d3.pointer(event);
            const adjustedX = x - margin.left;
            if (adjustedX >= 0 && adjustedX <= container._d3Timeline.width) {{
                const tick = Math.round(container._d3Timeline.xScale.invert(adjustedX));
                // Dispatch custom event for Leptos to handle
                const seekEvent = new CustomEvent('timeline-seek', {{ detail: {{ tick }} }});
                container.dispatchEvent(seekEvent);
            }}
        }});
    "#
    );

    let _ = js_sys::eval(&js_code);

    // Set up event listener for seek events
    let _container_clone = container.clone();
    let closure = Closure::wrap(Box::new(move |event: web_sys::Event| {
        if let Ok(custom_event) = event.dyn_into::<web_sys::CustomEvent>() {
            if let Ok(detail) = custom_event.detail().dyn_into::<js_sys::Object>() {
                if let Ok(tick_value) = js_sys::Reflect::get(&detail, &"tick".into()) {
                    if let Some(tick) = tick_value.as_f64() {
                        web_sys::console::log_1(&format!("Seek to tick: {}", tick).into());
                    }
                }
            }
        }
    }) as Box<dyn Fn(_)>);

    let _ = container
        .add_event_listener_with_callback("timeline-seek", closure.as_ref().unchecked_ref());
    closure.forget(); // Keep the closure alive
}

fn update_timeline_data(events: &[TimelineEvent], _current_tick: u64) {
    let events_json = serde_json::to_string(events).unwrap_or_default();
    let js_code = format!(
        r#"
        const container = document.querySelector('.d3-timeline-container');
        if (!container || !container._d3Timeline) return;

        const timeline = container._d3Timeline;
        const events = {};

        if (events.length === 0) return;

        // Update scales
        const maxTick = Math.max(...events.map(e => e.timestamp));
        timeline.xScale.domain([0, maxTick]);

        // Update X axis
        timeline.g.select(".x-axis")
            .call(d3.axisBottom(timeline.xScale).tickFormat(d => d + "s"));

        // Create event groups
        const eventGroups = timeline.g.selectAll(".event-group")
            .data(events, d => d.id);

        // Remove old events
        eventGroups.exit().remove();

        // Add new events
        const newGroups = eventGroups.enter()
            .append("g")
            .attr("class", "event-group");

        // Add event circles
        newGroups.append("circle")
            .attr("r", 4)
            .attr("fill", d => {{
                switch(d.event_type) {{
                    case "NodeStart": return "var(--color-success)";
                    case "KeyGen": return "var(--color-primary)";
                    case "NodeStop": return "var(--color-error)";
                    default: return "var(--color-secondary)";
                }}
            }})
            .attr("stroke", "white")
            .attr("stroke-width", 1);

        // Add event labels
        newGroups.append("text")
            .attr("dy", -8)
            .attr("text-anchor", "middle")
            .attr("font-size", "10px")
            .attr("fill", "var(--text-secondary)")
            .text(d => d.event_type);

        // Update positions for all events
        timeline.g.selectAll(".event-group")
            .attr("transform", d => `translate(${{timeline.xScale(d.timestamp)}}, ${{timeline.height / 2}})`);

        // Add tooltips
        timeline.g.selectAll(".event-group circle")
            .on("mouseover", function(event, d) {{
                const tooltip = timeline.g.append("g")
                    .attr("class", "tooltip")
                    .attr("transform", `translate(${{timeline.xScale(d.timestamp)}}, ${{timeline.height / 2 - 20}})`);
                    
                tooltip.append("rect")
                    .attr("x", -50)
                    .attr("y", -15)
                    .attr("width", 100)
                    .attr("height", 15)
                    .attr("fill", "var(--bg-primary)")
                    .attr("stroke", "var(--border-light)")
                    .attr("rx", 3);
                    
                tooltip.append("text")
                    .attr("text-anchor", "middle")
                    .attr("font-size", "10px")
                    .attr("fill", "var(--text-primary)")
                    .text(d.description);
            }})
            .on("mouseout", function() {{
                timeline.g.selectAll(".tooltip").remove();
            }});
    "#,
        events_json
    );

    let _ = js_sys::eval(&js_code);
}

fn update_current_position(tick: u64) {
    let js_code = format!(
        r#"
        const container = document.querySelector('.d3-timeline-container');
        if (!container || !container._d3Timeline) return;

        const timeline = container._d3Timeline;
        const x = timeline.xScale({});

        timeline.g.select(".current-position")
            .attr("x1", x)
            .attr("x2", x)
            .style("opacity", 1);
    "#,
        tick
    );

    let _ = js_sys::eval(&js_code);
}
