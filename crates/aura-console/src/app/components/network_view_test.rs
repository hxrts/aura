use leptos::prelude::*;

#[component]
pub fn NetworkViewTest() -> impl IntoView {
    log::info!("NetworkViewTest component rendering");

    view! {
        <div class="p-8 bg-yellow-200 border-4 border-yellow-500 rounded-lg">
            <h2 class="text-2xl font-bold text-yellow-900">"TEST: NetworkView Replacement"</h2>
            <p class="text-yellow-800">"If you can see this bright yellow box, then component rendering works."</p>
            <p class="text-yellow-800">"The issue is likely in the original NetworkView component logic."</p>

            <div class="mt-4 p-4 bg-white rounded">
                <h3 class="font-semibold">"Mock Data Test:"</h3>
                <ul class="list-disc ml-4">
                    <li>"Alice (Honest, Online)"</li>
                    <li>"Bob (Honest, Online)"</li>
                    <li>"Charlie (Byzantine, Error)"</li>
                    <li>"Dave (Observer, Syncing)"</li>
                </ul>
            </div>
        </div>
    }
}
