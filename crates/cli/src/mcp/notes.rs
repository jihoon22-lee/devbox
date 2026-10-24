use super::{Server, protocol::*, tools::tool};
use devbox_notes::{AppendFormat, AppendNote, AppendTarget, NotePath, NotesAppend, NotesRead};
use devbox_search::{NotesQuery, SearchNotes};

pub(super) fn tools(write: bool) -> Vec<Tool> {
    let mut out = vec![
        tool(
            "devbox_notes_search",
            "노트 본문에서 검색합니다(최대 20개).",
            &["text"],
        ),
        tool(
            "devbox_note_read",
            "노트 보관함 안의 노트를 읽습니다(최대 2 MiB).",
            &["path"],
        ),
    ];
    if write {
        let mut add = tool(
            "devbox_note_add",
            "받은 노트함 또는 오늘의 일일 기록에 내용을 덧붙입니다.",
            &["text"],
        );
        add.input_schema
            .properties
            .insert("daily", PropertySchema::Boolean);
        out.push(add);
    }
    out
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Search {
    text: String,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Add {
    text: String,
    #[serde(default)]
    daily: bool,
}
impl Server {
    pub(super) async fn call_note_tool(
        &mut self,
        call: CallParams,
    ) -> Result<ToolResult, RpcError> {
        match call.name.as_str() {
            "devbox_notes_search" => {
                let p: Search = params(call.arguments.as_deref())?;
                match self
                    .rpc::<SearchNotes>(&NotesQuery {
                        text: p.text,
                        limit: 20,
                    })
                    .await
                {
                    Ok(result) => ToolResult::json(result),
                    Err(error) => Ok(error),
                }
            }
            "devbox_note_read" => {
                let p: NotePath = params(call.arguments.as_deref())?;
                match self.rpc::<NotesRead>(&p).await {
                    Ok(result) => ToolResult::json(result),
                    Err(error) => Ok(error),
                }
            }
            "devbox_note_add" => {
                // A hidden tool is still refused on direct invocation, before any daemon mutation.
                if !self.write {
                    return Ok(ToolResult::error(
                        "MCP 쓰기 도구가 꺼져 있습니다. [mcp] write = true 설정이 필요합니다.",
                    ));
                }
                let p: Add = params(call.arguments.as_deref())?;
                match self
                    .rpc::<NotesAppend>(&AppendNote {
                        target: if p.daily {
                            AppendTarget::Daily
                        } else {
                            AppendTarget::Inbox
                        },
                        text: p.text,
                        format: AppendFormat::Plain,
                    })
                    .await
                {
                    Ok(result) => ToolResult::json(result),
                    Err(error) => Ok(error),
                }
            }
            _ => Err(ErrorKind::InvalidParams.into()),
        }
    }
}
