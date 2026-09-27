//! Synthetic A0 release gate. Run only inside Omabox with the isolated local Teams
//! server on port 18788. Uses real gateway routing and required receipt transport.
use conduit_lib::{registry, teams};
use serde_json::{json, Value};
use std::process::Command;

#[test]
#[ignore = "requires private Omabox HOME/keyring and synthetic Teams on 18788"]
fn managed_call_reaches_teams_with_raw_identity() {
    assert_eq!(
        std::env::var("HOME").unwrap(),
        "/home/sbx",
        "run inside Omabox"
    );
    let _lock = registry::data_dir_test_lock();
    let dir = std::path::PathBuf::from("/home/sbx/activation-a0");
    std::fs::create_dir_all(&dir).unwrap();
    let _override = registry::DataDirOverride::set(&dir);
    let api = "http://127.0.0.1:18788";
    let created: Value = ureq::post(&format!("{api}/teams"))
        .set("authorization", "Bearer activation-synthetic-bootstrap")
        .send_json(json!({"name":"A0 synthetic identity"}))
        .unwrap()
        .into_json()
        .unwrap();
    let team = created["team_id"].as_str().unwrap();
    let auth = format!("Bearer {}", created["admin_token"].as_str().unwrap());
    let fixture = "/home/sbx/activation-mcp.py";
    std::fs::write(fixture, r#"import sys,json
for line in sys.stdin:
 r=json.loads(line);m=r.get('method');i=r.get('id')
 if i is None:continue
 if m=='initialize':v={'protocolVersion':'2024-11-05','capabilities':{'tools':{}},'serverInfo':{'name':'synthetic-echo','version':'1'}}
 elif m=='tools/list':v={'tools':[{'name':'echo','description':'Read-only synthetic greeting','inputSchema':{'type':'object','properties':{'fail':{'type':'boolean'}}}}]}
 elif m=='tools/call':v={'content':[{'type':'text','text':'Synthetic result'}],'isError':r.get('params',{}).get('arguments',{}).get('fail',False)}
 else:v={}
 print(json.dumps({'jsonrpc':'2.0','id':i,'result':v}),flush=True)
"#).unwrap();
    ureq::put(&format!("{api}/teams/{team}/config")).set("authorization", &auth)
        .send_json(json!({"base_version":0,"config":{"servers":[{"id":"audit-echo", "name":"Synthetic echo", "transport":"stdio", "command":"python3", "args":[fixture]}]}})).unwrap();
    let invite: Value = ureq::post(&format!("{api}/teams/{team}/invites"))
        .set("authorization", &auth)
        .send_json(json!({"role":"member"}))
        .unwrap()
        .into_json()
        .unwrap();
    teams::connect(
        api,
        invite["invite_code"].as_str().unwrap(),
        Some("Synthetic member"),
    )
    .unwrap();
    registry::update(|r| {
        let id = r
            .servers
            .iter()
            .find(|s| s.source.as_deref() == Some(format!("team:{team}").as_str()))
            .unwrap()
            .id
            .clone();
        assert_eq!(id, "team_audit-echo");
        // Explicit synthetic review. The final usability gate must use the actual UI.
        r.profiles[0].enabled_server_ids.push(id);
        Ok(())
    })
    .unwrap();
    let client = r#"import subprocess,json,select,time,sys
p=subprocess.Popen([sys.argv[1]],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=open('/home/sbx/activation-gateway.log','w'),text=True,bufsize=1)
def rpc(i,m,params):
 p.stdin.write(json.dumps({'jsonrpc':'2.0','id':i,'method':m,'params':params})+'\n');p.stdin.flush();deadline=time.time()+40
 while time.time()<deadline:
  if select.select([p.stdout],[],[],1)[0]:
   line=p.stdout.readline()
   if not line:break
   r=json.loads(line)
   if r.get('id')==i:return r
 raise RuntimeError('timeout '+m)
try:
 rpc(1,'initialize',{'protocolVersion':'2024-11-05','capabilities':{},'clientInfo':{'name':'activation-protocol-test','version':'1'}})
 p.stdin.write(json.dumps({'jsonrpc':'2.0','method':'notifications/initialized'})+'\n');p.stdin.flush()
 rpc(2,'tools/call',{'name':'toolport_search_tools','arguments':{'query':'echo'}})
 for i,fail in [(3,False),(4,True)]:
  r=rpc(i,'tools/call',{'name':'toolport_call_tool','arguments':{'name':'team_audit_echo__echo','arguments':{'fail':fail}}})
  print(json.dumps(r),flush=True)
  assert r['result'].get('isError',False)==fail,r
finally:p.terminate();p.wait(timeout=10)
"#;
    let output = Command::new("python3")
        .args(["-c", client, &std::env::var("ACTIVATION_GATEWAY").unwrap()])
        .env("TOOLPORT_DATA_DIR", &dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    teams::sync_now().unwrap();
    let evidence: Value = ureq::get(&format!("{api}/teams/{team}/activation"))
        .set("authorization", &auth)
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    let row = &evidence["devices"][0];
    assert!(row["firstSuccessAt"].is_i64(), "{evidence}");
    let counter = &row["receipt"]["counters"]["audit-echo"];
    assert_eq!(counter["successes"], 1);
    assert_eq!(counter["failures"], 1);
    teams::sync_now().unwrap();
    let retried: Value = ureq::get(&format!("{api}/teams/{team}/activation"))
        .set("authorization", &auth)
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(
        retried["devices"][0]["firstSuccessAt"],
        row["firstSuccessAt"]
    );
    std::fs::write(
        "/home/sbx/a0-evidence.json",
        serde_json::to_string_pretty(&retried).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "requires synthetic authenticated portal fixture in private Omabox"]
fn portal_member_connect_keeps_the_authenticated_seat() {
    assert_eq!(std::env::var("HOME").unwrap(), "/home/sbx");
    let _lock = registry::data_dir_test_lock();
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string("/home/sbx/portal-connect.json").unwrap()).unwrap();
    let dir = std::path::PathBuf::from("/home/sbx/activation-member");
    std::fs::create_dir_all(&dir).unwrap();
    let _override = registry::DataDirOverride::set(dir);
    teams::connect("http://127.0.0.1:18788", fixture["connectCode"].as_str().unwrap(), None).unwrap();
    teams::sync_now().unwrap();
    let connection = registry::load().unwrap().team.unwrap();
    assert_eq!(connection.team_id, fixture["teamId"].as_str().unwrap());
    assert_eq!(connection.role, "member");
    assert_eq!(connection.account_linked, Some(true));
    assert_eq!(connection.team_name.as_deref(), Some("Activation Acme"));
    let token = teams::load_token().unwrap().unwrap();
    let me: Value = ureq::get(&format!("http://127.0.0.1:18788/teams/{}/me", connection.team_id))
        .set("authorization", &format!("Bearer {token}")).call().unwrap().into_json().unwrap();
    assert_eq!(me["member_id"], fixture["memberId"]);
}
