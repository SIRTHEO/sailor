use super::*;
use serde_json::json;

const AT_LAUNCH: &str = "2026-09-24T10:00:00Z";
const LAUNCHED_AT: i64 = 1_790_244_000;

/// The words the shipped descriptor declares, so these read what the product reads.
fn words() -> OpenInRecord {
    let shipped: Value =
        serde_json::from_str(toolbox::descriptor::BUILTIN).expect("the descriptor parses");
    let declared = shipped["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|tool| tool["free_when"].get("and_leaves_nothing_open_in_its_record"))
        .expect("a line declares what its record leaves open");
    serde_json::from_value(declared.clone()).expect("the words read")
}

fn prompt(text: &str) -> Value {
    json!({"type": "user", "message": {"role": "user", "content": text}})
}

fn launch(id: &str, task: &str) -> Vec<Value> {
    vec![
        json!({"type": "assistant", "timestamp": AT_LAUNCH, "message": {"content": [
            {"type": "tool_use", "id": id, "name": "Agent", "input": {"description": "review the commit"}}
        ]}}),
        json!({"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": id, "content": [{"type": "text",
             "text": format!("Async agent launched successfully.\nagentId: {task} (internal ID)")}]}
        ]}}),
    ]
}

fn report(id: &str) -> Value {
    prompt(&format!("<task-notification>\n<task-id>x</task-id>\n<tool-use-id>{id}</tool-use-id>\n<status>completed</status>"))
}

fn open(rows: &[Value], now: i64) -> Vec<Open> {
    let text: String = rows.iter().map(|row| format!("{row}\n")).collect();
    read(text.as_bytes(), &words(), now).expect("the record reads")
}

/// **AN AGENT IN FLIGHT IS OPEN UNTIL IT REPORTS.** Both directions, because
/// the claim is the difference the report makes.
#[test]
fn an_agent_launched_in_the_background_is_open_until_it_reports() {
    let mut rows = vec![prompt("review it")];
    rows.extend(launch("toolu_1", "a1b2"));
    assert_eq!(
        open(&rows, LAUNCHED_AT + 60),
        [Open::Launched {
            what: "review the commit".to_owned()
        }]
    );
    rows.push(report("toolu_1"));
    assert_eq!(open(&rows, LAUNCHED_AT + 60), []);
}

/// A report reaches the record as a queued command too, and that counts.
#[test]
fn a_report_that_arrives_as_a_queued_command_closes_the_launch() {
    let mut rows = vec![prompt("review it")];
    rows.extend(launch("toolu_1", "a1b2"));
    rows.push(json!({"type": "attachment", "attachment": {
        "type": "queued_command", "prompt": "<task-notification>\n<tool-use-id>toolu_1</tool-use-id>"}}));
    assert_eq!(open(&rows, LAUNCHED_AT + 60), []);
}

#[test]
fn an_agent_stopped_by_hand_is_not_open() {
    let mut rows = vec![prompt("review it")];
    rows.extend(launch("toolu_1", "a1b2"));
    rows.push(json!({"type": "assistant", "message": {"content": [
        {"type": "tool_use", "id": "toolu_2", "name": "TaskStop", "input": {"task_id": "a1b2"}}]}}));
    rows.push(json!({"type": "user", "message": {"content": [
        {"type": "tool_result", "tool_use_id": "toolu_2", "content": "stopped"}]}}));
    assert_eq!(open(&rows, LAUNCHED_AT + 60), []);
}

/// Past the declared age a silent launch is lost: waiting on it would keep the
/// session full forever.
#[test]
fn a_launch_silent_past_the_declared_age_is_lost_not_open() {
    let mut rows = vec![prompt("review it")];
    rows.extend(launch("toolu_1", "a1b2"));
    let lost = words().a_launch_is_lost_after_seconds as i64;
    assert_eq!(open(&rows, LAUNCHED_AT + lost).len(), 1);
    assert_eq!(open(&rows, LAUNCHED_AT + lost + 1), []);
}

/// **A REPORT QUOTED IN A TOOL'S OUTPUT IS NOT A REPORT.** A session that
/// greps its own record prints the tags, and that must not close a launch.
#[test]
fn a_report_quoted_inside_a_tools_answer_does_not_close_the_launch() {
    let mut rows = vec![prompt("review it")];
    rows.extend(launch("toolu_1", "a1b2"));
    rows.push(json!({"type": "assistant", "message": {"content": [
        {"type": "tool_use", "id": "toolu_3", "name": "Bash", "input": {"command": "grep"}}]}}));
    rows.push(json!({"type": "user", "message": {"content": [
        {"type": "tool_result", "tool_use_id": "toolu_3",
         "content": "<task-notification><tool-use-id>toolu_1</tool-use-id>"}]}}));
    assert_eq!(open(&rows, LAUNCHED_AT + 60).len(), 1);
}

/// The last turn's call with no answer is a question or a permission still
/// waiting; an earlier one was interrupted and the session went on.
#[test]
fn only_the_last_turns_unanswered_call_is_open() {
    let asked = json!({"type": "assistant", "message": {"content": [
        {"type": "tool_use", "id": "toolu_q", "name": "AskUserQuestion", "input": {}}]}});
    let rows = vec![prompt("choose"), asked.clone()];
    assert_eq!(
        open(&rows, LAUNCHED_AT),
        [Open::Unanswered {
            call: "AskUserQuestion".to_owned()
        }]
    );
    let rows = vec![prompt("choose"), asked, prompt("never mind, go on")];
    assert_eq!(open(&rows, LAUNCHED_AT), []);
}

#[test]
fn a_message_queued_and_not_delivered_is_open() {
    let rows = vec![
        prompt("go"),
        json!({"type": "queue-operation", "operation": "enqueue", "content": "and then this"}),
        json!({"type": "queue-operation", "operation": "enqueue", "content": "and this"}),
        json!({"type": "queue-operation", "operation": "dequeue"}),
    ];
    assert_eq!(open(&rows, LAUNCHED_AT), [Open::Queued { count: 1 }]);
}

/// One operation hands on every queued message at once.
#[test]
fn a_queue_emptied_at_once_leaves_nothing_queued() {
    let rows = vec![
        prompt("go"),
        json!({"type": "queue-operation", "operation": "enqueue", "content": "and then this"}),
        json!({"type": "queue-operation", "operation": "enqueue", "content": "and this"}),
        json!({"type": "queue-operation", "operation": "popAll"}),
    ];
    assert_eq!(open(&rows, LAUNCHED_AT), []);
}

fn queued(operation: &str, at: &str, message: &str) -> Value {
    json!({"type": "queue-operation", "operation": operation, "timestamp": at, "content": message})
}

/// A message nothing took, past the declared age, is lost: a notification the
/// session never received would otherwise keep it full forever.
#[test]
fn a_queued_message_nothing_took_is_lost_past_the_declared_age() {
    let rows = vec![
        prompt("go"),
        queued("enqueue", AT_LAUNCH, "<task-notification>"),
        prompt("and later this"),
    ];
    let lost = words()
        .a_queued_message_is_lost_after_seconds
        .expect("the shipped line declares an age") as i64;
    assert_eq!(open(&rows, LAUNCHED_AT + lost), [Open::Queued { count: 1 }]);
    assert_eq!(open(&rows, LAUNCHED_AT + lost + 1), []);
}

/// A removal takes the message it names, so the one still waiting keeps its own age.
#[test]
fn a_removal_takes_the_message_it_names() {
    let rows = vec![
        prompt("go"),
        queued("enqueue", AT_LAUNCH, "and then this"),
        queued("enqueue", "2026-09-24T11:00:00Z", "and this"),
        queued("remove", "2026-09-24T11:00:01Z", "and then this"),
    ];
    let lost = words()
        .a_queued_message_is_lost_after_seconds
        .unwrap_or_default() as i64;
    assert_eq!(
        open(&rows, LAUNCHED_AT + lost + 1),
        [Open::Queued { count: 1 }]
    );
}

#[test]
fn a_record_that_cannot_be_read_is_not_a_record_with_nothing_open() {
    assert_eq!(
        open_in("/nonexistent/record.jsonl", &words(), LAUNCHED_AT),
        None
    );
}
