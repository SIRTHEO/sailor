//! A shipped flow that changes says so by its version.
//!
//! A person's copy of a shipped flow names the version it replaces, and is
//! told stale once the shipped one rises past it. That holds only if every
//! change to a shipped flow raises its version: this table is the memory of
//! what each version read, and a change without a new version falls here.

use flow::starters::version_of;
use flow::system::FLOWS;
use flow::FlowFile;

/// Each shipped flow, the version it declares, and the digest of its text.
const SHIPPED_VERSIONS_TODAY: &[(&str, u32, &str)] = &[
    (
        "ask-for-a-mandate",
        1,
        "aee53cc37c450dc5795ebda7afad470621d04ebebf0de255be6f386c81ecca46",
    ),
    (
        "build-the-bench",
        1,
        "eb51987a62dbc682b7c9c6f1213ebf16feefcf28fbd47bb8ffefe3ee429bd93f",
    ),
    (
        "close-the-work",
        1,
        "14d18d5ef0179aefad1ced097702b7901555444aa8cb2d6ef1baa318057e2fd0",
    ),
    (
        "consolidate-memories",
        1,
        "28e9b92c6f2194cf6453d2a244364382e4906ef9cbcdd2a51f81b13d5bda9f76",
    ),
    (
        "consult-a-strong-model",
        1,
        "63595688709ce399df03692ddde0fc214ae0c9fe273548e071d7ef080def1535",
    ),
    (
        "cut-a-release",
        1,
        "bea42e8062de9827c78e6334ccc74f1501ec0d2b5a46fe89d4bdcfefa2c85680",
    ),
    (
        "dispatch-the-work",
        1,
        "074c3843c203c87e4d50d607d3e571089fc089a416c2bc6873ad1f74f0e69b77",
    ),
    (
        "draft-a-flow",
        1,
        "00d5109992bcc24917c75dd063e1fb59135b4ea0a93f7a2d1b6d6e7e3f4f42e0",
    ),
    (
        "empty-a-session-that-handed-on",
        1,
        "f2d17edbce09a85a0e14be7717a83ca1117f0c6735a174552e4d65b5e3e2bb4f",
    ),
    (
        "find-the-dead-powers",
        1,
        "cab38ffcffe23498d2e5d725a20225db8124c85d5164d6d44b3e15086f4591b6",
    ),
    (
        "integrate-on-the-trunk",
        1,
        "2c52aecd42dea3d60b6bf91281b783bc3b00f0976385b9af0c2c4e642e39103b",
    ),
    (
        "judge-a-change",
        1,
        "824ec3976d56fe3fd90763b7cdeadb0ca945b6ecff54dcfbf06c44f80fe8fd24",
    ),
    (
        "measure-the-baseline",
        1,
        "8a3f6f1e89a5cb93a514851ea7f1fcec97e43cd87e03278c617357f33f510637",
    ),
    (
        "migrate-to-sailor",
        1,
        "bb28fac30cd6860a7c889ae256b49a1b99c42967195088af0e1c9a2ae4e7f178",
    ),
    (
        "notice-a-divergence",
        1,
        "cd85f8c4c8b752ac2edf62ca8eeaebe381cdce7cb54f71ff0d6d372b5637f64d",
    ),
    (
        "one-agent-on-one-task",
        1,
        "f5e2dd53d5c4a8c3b70077bf83180d6e8ffddd0a92f1f970e8cc6aa89132c312",
    ),
    (
        "open-the-draft-pull-request",
        1,
        "7e1e1b07d1fdf8b3dabc3bb580ade132560a58400131ec906816ab821f5b4275",
    ),
    (
        "price-every-call",
        1,
        "366c556e9abaaa02b6f38c164e13e5941ea37a58915f6913a265e4263e9a1b08",
    ),
    (
        "put-the-crew-to-work",
        1,
        "13883315634f257615852367ac62736b9d6c702a4b70c20105f4d2908c20bb6b",
    ),
    (
        "recall-a-decision",
        1,
        "4451be42d4859c7de289e38a7a9939aa42d2e67c44a00b38bfb4df534f50d608",
    ),
    (
        "remember-in-the-graph",
        1,
        "84a4f3b5897e71d549fcd94b0fdbf24b5c79dbaa73528a400f1f306306144ea3",
    ),
    (
        "review-a-pinned-commit",
        1,
        "b2ce0486823e87c0dd45d28cc3771c26e20daac2f84d18c42b17cf839da60974",
    ),
    (
        "run-one-bench-task",
        1,
        "464034d1fc97464630262f6ecd688dd32ae208f618102da79cdacec3a84e3dbe",
    ),
    (
        "sweep-the-tree",
        1,
        "53702330064c956ae49490a998d03528d330f0b429977ba83b856dc88937b71e",
    ),
    (
        "take-the-next-fault",
        1,
        "2b77f84566218d23e0f44f44e0d8b938278b78d3a2e8e09d19f15cfcb6d07939",
    ),
    (
        "take-the-next-work",
        1,
        "2917702461d3ef27a4cc7d92205e0f765439b8c1abe852a30890fe2c29bea591",
    ),
    (
        "validate-a-bench-task",
        1,
        "49a238000e227a958ebdceee4e5144caca0f52ce0486ef22b57ed7a680f014a3",
    ),
    (
        "watch-the-crew",
        1,
        "e02066260b8cb3c74738afae595d0a79f731c6df2d0558c1ee853a27e15aec69",
    ),
    (
        "write-down-what-broke",
        1,
        "47514401f2ceefa886c947f9815cc6199df5758b5f2ac298beeb28a55ff9f90f",
    ),
    (
        "what-this-machine-has",
        1,
        "483f97204d0cd4e1d473ef3478f933d53735045f579623ce63a75ddb70fb98d4",
    ),
];

fn declared(text: &str) -> Option<u32> {
    serde_json::from_str::<FlowFile>(text).ok()?.version
}

/// What is wrong between the table and the flows, one line each.
fn apart(table: &[(&str, u32, &str)], flows: &[(&str, &str)]) -> Vec<String> {
    let mut said = Vec::new();
    for (name, text) in flows {
        let digest = version_of(text);
        let version = declared(text).unwrap_or(0);
        let row = format!("(\"{name}\", {version}, \"{digest}\")");
        match table.iter().find(|(listed, _, _)| listed == name) {
            None => said.push(format!("{name} is shipped and has no row: write {row}")),
            Some((_, listed, known)) if *known == digest => {
                if *listed != version {
                    said.push(format!("{name}: the row says {listed}, write {row}"));
                }
            }
            Some((_, listed, _)) if version <= *listed => said.push(format!(
                "{name} changed and still says version {version}: raise it to {} in the \
                 flow, then write its row",
                listed + 1
            )),
            Some(_) => said.push(format!("{name} rose: write {row}")),
        }
    }
    for (listed, _, _) in table {
        if !flows.iter().any(|(name, _)| name == listed) {
            said.push(format!("{listed} is no longer shipped: take its row out"));
        }
    }
    said
}

#[test]
fn every_shipped_flow_reads_as_the_version_it_declares() {
    let said = apart(SHIPPED_VERSIONS_TODAY, FLOWS);
    assert!(said.is_empty(), "{}", said.join("\n"));
}

/// The control: a flow edited without a new version is caught, and the
/// same flow with its version raised asks only for its row.
#[test]
fn a_change_without_a_new_version_is_caught() {
    let (name, text) = FLOWS[0];
    let (_, version, _) = SHIPPED_VERSIONS_TODAY
        .iter()
        .find(|(listed, _, _)| *listed == name)
        .expect("the first shipped flow has a row");
    let edited = text.replacen("\"description\": \"", "\"description\": \"edited. ", 1);
    assert_ne!(edited, text, "the edit must change the text");
    let about = |flows: &[(&str, &str)]| -> Vec<String> {
        apart(SHIPPED_VERSIONS_TODAY, flows)
            .into_iter()
            .filter(|line| line.starts_with(name))
            .collect()
    };
    let said = about(&[(name, &edited)]);
    assert!(
        said.iter().any(|line| line.contains("raise it to")),
        "{said:?}"
    );

    let raised = edited.replacen(
        &format!("\"version\": {version},"),
        &format!("\"version\": {},", version + 1),
        1,
    );
    let said = about(&[(name, &raised)]);
    assert!(
        !said.is_empty() && said.iter().all(|line| line.contains("rose: write")),
        "{said:?}"
    );
}
