// Populate the sidebar
//
// This is a script, and not included directly in the page, to control the total size of the book.
// The TOC contains an entry for each page, so if each page includes a copy of the TOC,
// the total size of the page becomes O(n**2).
class MDBookSidebarScrollbox extends HTMLElement {
    constructor() {
        super();
    }
    connectedCallback() {
        this.innerHTML = '<ol class="chapter"><li class="chapter-item expanded "><a href="000_project_overview.html"><strong aria-hidden="true">1.</strong> Project Overview</a></li><li class="chapter-item expanded "><a href="001_system_architecture.html"><strong aria-hidden="true">2.</strong> Aura System Architecture</a></li><li class="chapter-item expanded "><a href="002_theoretical_model.html"><strong aria-hidden="true">3.</strong> Theoretical Model</a></li><li class="chapter-item expanded "><a href="003_information_flow_contract.html"><strong aria-hidden="true">4.</strong> Privacy and Information Flow</a></li><li class="chapter-item expanded "><a href="004_distributed_systems_contract.html"><strong aria-hidden="true">5.</strong> Distributed Systems Contract</a></li><li class="chapter-item expanded "><a href="005_system_invariants.html"><strong aria-hidden="true">6.</strong> Aura System Invariants</a></li><li class="chapter-item expanded "><a href="100_authority_and_identity.html"><strong aria-hidden="true">7.</strong> Authority and Identity</a></li><li class="chapter-item expanded "><a href="101_accounts_and_commitment_tree.html"><strong aria-hidden="true">8.</strong> Accounts and Commitment Tree</a></li><li class="chapter-item expanded "><a href="102_journal.html"><strong aria-hidden="true">9.</strong> Journal</a></li><li class="chapter-item expanded "><a href="103_relational_contexts.html"><strong aria-hidden="true">10.</strong> Relational Contexts</a></li><li class="chapter-item expanded "><a href="104_consensus.html"><strong aria-hidden="true">11.</strong> Consensus</a></li><li class="chapter-item expanded "><a href="105_identifiers_and_boundaries.html"><strong aria-hidden="true">12.</strong> Identifiers and Boundaries</a></li><li class="chapter-item expanded "><a href="106_effect_system_and_runtime.html"><strong aria-hidden="true">13.</strong> Effect System and Runtime</a></li><li class="chapter-item expanded "><a href="107_mpst_and_choreography.html"><strong aria-hidden="true">14.</strong> Multi-party Session Types and Choreography</a></li><li class="chapter-item expanded "><a href="108_transport_and_information_flow.html"><strong aria-hidden="true">15.</strong> Transport and Information Flow</a></li><li class="chapter-item expanded "><a href="109_authorization.html"><strong aria-hidden="true">16.</strong> Authorization</a></li><li class="chapter-item expanded "><a href="110_rendezvous.html"><strong aria-hidden="true">17.</strong> Rendezvous Architecture</a></li><li class="chapter-item expanded "><a href="110_state_reduction.html"><strong aria-hidden="true">18.</strong> State Reduction Flows</a></li><li class="chapter-item expanded "><a href="111_maintenance.html"><strong aria-hidden="true">19.</strong> Maintenance Guidelines</a></li><li class="chapter-item expanded "><a href="112_amp.html"><strong aria-hidden="true">20.</strong> Aura Messaging Protocol (AMP)</a></li><li class="chapter-item expanded "><a href="114_frost_pipelining_optimization.html"><strong aria-hidden="true">21.</strong> FROST Pipelined Commitment Optimization</a></li><li class="chapter-item expanded "><a href="801_hello_world_guide.html"><strong aria-hidden="true">22.</strong> Hello World Guide</a></li><li class="chapter-item expanded "><a href="802_core_systems_guide.html"><strong aria-hidden="true">23.</strong> Core Systems Guide: Time Domain Selection</a></li><li class="chapter-item expanded "><a href="803_coordination_guide.html"><strong aria-hidden="true">24.</strong> Coordination Systems Guide</a></li><li class="chapter-item expanded "><a href="804_advanced_coordination_guide.html"><strong aria-hidden="true">25.</strong> Advanced Choreography Guide</a></li><li class="chapter-item expanded "><a href="805_development_patterns.html"><strong aria-hidden="true">26.</strong> Development Patterns and Workflows</a></li><li class="chapter-item expanded "><a href="805_testing_guide.html"><strong aria-hidden="true">27.</strong> Testing Guide</a></li><li class="chapter-item expanded "><a href="806_simulation_guide.html"><strong aria-hidden="true">28.</strong> Simulation Guide</a></li><li class="chapter-item expanded "><a href="807_maintenance_ota_guide.html"><strong aria-hidden="true">29.</strong> Maintenance and OTA Guide</a></li><li class="chapter-item expanded "><a href="999_project_structure.html"><strong aria-hidden="true">30.</strong> Aura Crate Structure and Dependency Graph</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="adr/014_guard.html"><strong aria-hidden="true">30.1.</strong> ADR-014: Pure Guard Evaluation with Asynchronous Effect Interpretation</a></li><li class="chapter-item expanded "><a href="adr/015_choreography_first_guards.html"><strong aria-hidden="true">30.2.</strong> ADR-015: Choreography-First Guard Architecture</a></li><li class="chapter-item expanded "><a href="blog/000_beyond_local_first.html"><strong aria-hidden="true">30.3.</strong> Beyond Local First</a></li><li class="chapter-item expanded "><a href="blog/001_group_messaging.html"><strong aria-hidden="true">30.4.</strong> Aura Messaging Protocol</a></li><li class="chapter-item expanded "><a href="blog/002_consensus.html"><strong aria-hidden="true">30.5.</strong> Aura Consensus</a></li><li class="chapter-item expanded "><a href="demo/cli_recovery.html"><strong aria-hidden="true">30.6.</strong> Aura CLI Recovery Demo (CLI + Simulator)</a></li></ol></li><li class="chapter-item expanded "><a href="privacy_checklist.html"><strong aria-hidden="true">31.</strong> Privacy-by-Design Checklist</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="quint/language_guide.html"><strong aria-hidden="true">31.1.</strong> Quint + Choreo</a></li></ol></li></ol>';
        // Set the current, active page, and reveal it if it's hidden
        let current_page = document.location.href.toString().split("#")[0].split("?")[0];
        if (current_page.endsWith("/")) {
            current_page += "index.html";
        }
        var links = Array.prototype.slice.call(this.querySelectorAll("a"));
        var l = links.length;
        for (var i = 0; i < l; ++i) {
            var link = links[i];
            var href = link.getAttribute("href");
            if (href && !href.startsWith("#") && !/^(?:[a-z+]+:)?\/\//.test(href)) {
                link.href = path_to_root + href;
            }
            // The "index" page is supposed to alias the first chapter in the book.
            if (link.href === current_page || (i === 0 && path_to_root === "" && current_page.endsWith("/index.html"))) {
                link.classList.add("active");
                var parent = link.parentElement;
                if (parent && parent.classList.contains("chapter-item")) {
                    parent.classList.add("expanded");
                }
                while (parent) {
                    if (parent.tagName === "LI" && parent.previousElementSibling) {
                        if (parent.previousElementSibling.classList.contains("chapter-item")) {
                            parent.previousElementSibling.classList.add("expanded");
                        }
                    }
                    parent = parent.parentElement;
                }
            }
        }
        // Track and set sidebar scroll position
        this.addEventListener('click', function(e) {
            if (e.target.tagName === 'A') {
                sessionStorage.setItem('sidebar-scroll', this.scrollTop);
            }
        }, { passive: true });
        var sidebarScrollTop = sessionStorage.getItem('sidebar-scroll');
        sessionStorage.removeItem('sidebar-scroll');
        if (sidebarScrollTop) {
            // preserve sidebar scroll position when navigating via links within sidebar
            this.scrollTop = sidebarScrollTop;
        } else {
            // scroll sidebar to current active section when navigating via "next/previous chapter" buttons
            var activeSection = document.querySelector('#sidebar .active');
            if (activeSection) {
                activeSection.scrollIntoView({ block: 'center' });
            }
        }
        // Toggle buttons
        var sidebarAnchorToggles = document.querySelectorAll('#sidebar a.toggle');
        function toggleSection(ev) {
            ev.currentTarget.parentElement.classList.toggle('expanded');
        }
        Array.from(sidebarAnchorToggles).forEach(function (el) {
            el.addEventListener('click', toggleSection);
        });
    }
}
window.customElements.define("mdbook-sidebar-scrollbox", MDBookSidebarScrollbox);
