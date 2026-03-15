//! Random thinking verbs for streaming status indicators.

/// Thinking verb pairs (present participle, past tense) shown during/after streaming.
pub const THINKING_VERBS: &[(&str, &str)] = &[
    ("Thinking", "Thought"),
    ("Reasoning", "Reasoned"),
    ("Pondering", "Pondered"),
    ("Analyzing", "Analyzed"),
    ("Cogitating", "Cogitated"),
    ("Contemplating", "Contemplated"),
    ("Deliberating", "Deliberated"),
    ("Evaluating", "Evaluated"),
    ("Processing", "Processed"),
    ("Reflecting", "Reflected"),
    ("Considering", "Considered"),
    ("Strategizing", "Strategized"),
    ("Deciphering", "Deciphered"),
    ("Investigating", "Investigated"),
    ("Synthesizing", "Synthesized"),
    ("Computing", "Computed"),
    ("Formulating", "Formulated"),
    ("Ruminating", "Ruminated"),
    ("Musing", "Mused"),
    ("Deducing", "Deduced"),
    ("Examining", "Examined"),
    ("Unraveling", "Unraveled"),
    ("Assembling", "Assembled"),
    ("Distilling", "Distilled"),
    ("Parsing", "Parsed"),
    ("Weighing", "Weighed"),
    ("Calibrating", "Calibrated"),
    ("Conjuring", "Conjured"),
    ("Orchestrating", "Orchestrated"),
    ("Extrapolating", "Extrapolated"),
];

/// Pick a random thinking verb pair using a simple time-based seed.
pub fn random_thinking_verb() -> (&'static str, &'static str) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as usize;
    THINKING_VERBS[nanos % THINKING_VERBS.len()]
}
