use crate::server::LSPServer;
use crate::sources::{LSPSupport, ParseData, Scope};
use log::info;
use ropey::RopeSlice;
use std::str;
use sv_parser::*;
use tower_lsp::lsp_types::*;
use trie_rs::{Trie, TrieBuilder};

impl LSPServer {
    pub fn completion(&self, params: CompletionParams) -> Option<CompletionResponse> {
        let doc = params.text_document_position;
        let file_id = self.srcs.get_id(doc.text_document.uri).to_owned();
        let data = self.srcs.get_parse_data(file_id).unwrap();
        let file = self.srcs.get_file(file_id).unwrap();
        Some(CompletionResponse::List(get_completion(
            file.text.line(doc.position.line as usize),
            data,
            doc.position,
            file.text.pos_to_byte(doc.position),
        )))
    }
}

fn get_identifiers(syntax_tree: &SyntaxTree) -> Vec<(String, usize)> {
    let mut idents: Vec<(String, usize)> = Vec::new();
    for node in syntax_tree {
        match node {
            RefNode::Identifier(x) => {
                let id = match x {
                    Identifier::SimpleIdentifier(x) => x.nodes.0,
                    Identifier::EscapedIdentifier(x) => x.nodes.0,
                };
                let id_str = syntax_tree.get_str(&id).unwrap();
                let idb = syntax_tree.get_origin(&id).unwrap().1;
                idents.push((id_str.to_owned(), idb));
            }
            _ => (),
        }
    }
    idents
}

pub fn get_scopes(syntax_tree: &SyntaxTree) -> Vec<Scope> {
    let mut scopes: Vec<Scope> = Vec::new();
    let identifiers = get_identifiers(&syntax_tree);

    fn build_trie(start: usize, end: usize, identifiers: &Vec<(String, usize)>) -> Trie<u8> {
        let mut builder = TrieBuilder::new();
        for id in identifiers {
            if id.1 >= start && id.1 <= end {
                builder.push(&id.0);
            }
        }
        builder.build()
    }

    for node in syntax_tree {
        match node {
            RefNode::ModuleDeclarationAnsi(x) => {
                // Declaration -> Keyword -> Locate
                let end = syntax_tree.get_origin(&x.nodes.3.nodes.0).unwrap().1;
                // Declaration -> Header -> ModuleKeyword
                let start = match &x.nodes.0.nodes.1 {
                    ModuleKeyword::Module(x) | ModuleKeyword::Macromodule(x) => x.nodes.0,
                };
                let start = syntax_tree.get_origin(&start).unwrap().1;
                // Declaration -> Header -> ModuleIdentifier -> Identifier
                let name = match &x.nodes.0.nodes.3.nodes.0 {
                    Identifier::SimpleIdentifier(x) => x.nodes.0,
                    Identifier::EscapedIdentifier(x) => x.nodes.0,
                };
                let name = syntax_tree.get_str(&name).unwrap();
                scopes.push(Scope {
                    name: name.to_owned(),
                    start,
                    end,
                    trie: build_trie(start, end, &identifiers),
                });
            }
            RefNode::ModuleDeclarationNonansi(x) => {
                // Declaration -> Keyword -> Locate
                let end = syntax_tree.get_origin(&x.nodes.3.nodes.0).unwrap().1;
                // Declaration -> Header -> ModuleKeyword
                let start = match &x.nodes.0.nodes.1 {
                    ModuleKeyword::Module(x) | ModuleKeyword::Macromodule(x) => x.nodes.0,
                };
                let start = syntax_tree.get_origin(&start).unwrap().1;
                // Declaration -> Header -> ModuleIdentifier -> Identifier
                let name = match &x.nodes.0.nodes.3.nodes.0 {
                    Identifier::SimpleIdentifier(x) => x.nodes.0,
                    Identifier::EscapedIdentifier(x) => x.nodes.0,
                };
                let name = syntax_tree.get_str(&name).unwrap();
                scopes.push(Scope {
                    name: name.to_owned(),
                    start,
                    end,
                    trie: build_trie(start, end, &identifiers),
                });
            }
            _ => (),
        }
    }
    scopes
}

pub fn get_completion(
    line: RopeSlice,
    data: &ParseData,
    pos: Position,
    bpos: usize,
) -> CompletionList {
    let token = get_completion_token(line, pos);
    let mut scopes: Vec<&Scope> = data
        .scopes
        .iter()
        .filter(|x| bpos >= x.start && bpos <= x.end)
        .collect();
    scopes.sort_by(|a, b| (a.end - a.start).cmp(&(b.end - b.start)));
    let scope = *scopes.get(0).unwrap();
    let results = scope.trie.predictive_search(&token);
    let results_in_str: Vec<&str> = results
        .iter()
        .map(|u8s| str::from_utf8(u8s).unwrap())
        .collect();
    CompletionList {
        is_incomplete: results_in_str.contains(&token.as_str()),
        items: results_in_str
            .iter()
            .map(|x| CompletionItem {
                label: (*x).to_owned(),
                kind: None,
                detail: None,
                documentation: None,
                deprecated: None,
                preselect: None,
                sort_text: None,
                filter_text: None,
                insert_text: None,
                insert_text_format: None,
                text_edit: None,
                additional_text_edits: None,
                command: None,
                data: None,
                tags: None,
            })
            .collect(),
    }
}

fn get_completion_token(line: RopeSlice, pos: Position) -> String {
    let mut token = String::new();
    let mut line_iter = line.chars();
    for _ in 0..(line.utf16_cu_to_char(pos.character as usize)) {
        line_iter.next();
    }
    let mut c = line_iter.prev();
    while !c.is_none() && c.unwrap().is_alphanumeric() {
        token.push(c.unwrap());
        c = line_iter.prev();
    }
    token.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ropey::Rope;

    #[test]
    fn test_get_completion_token() {
        let text = Rope::from_str("abc abc.cba defg");
        let mut result = get_completion_token(
            text.line(0),
            Position {
                line: 0,
                character: 3,
            },
        );
        assert_eq!(&result, "abc");
        result = get_completion_token(
            text.line(0),
            Position {
                line: 0,
                character: 11,
            },
        );
        assert_eq!(&result, "cba");
        result = get_completion_token(
            text.line(0),
            Position {
                line: 0,
                character: 16,
            },
        );
        assert_eq!(&result, "defg");
    }

    #[test]
    fn test_completion() {
        let mut server = LSPServer::new();
        let uri = Url::parse("file:///test.sv").unwrap();
        let text = r#"module test;
    logic abc;
    logic abcd;

endmodule
"#;
        let open_params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "systemverilog".to_owned(),
                version: 0,
                text: text.to_owned(),
            },
        };
        server.did_open(open_params);

        let change_params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: Some(3),
            },
            content_changes: vec![
                TextDocumentContentChangeEvent {
                    range: Some(Range {
                        start: Position {
                            line: 3,
                            character: 0,
                        },
                        end: Position {
                            line: 3,
                            character: 0,
                        },
                    }),
                    range_length: None,
                    text: "\n".to_owned(),
                },
                TextDocumentContentChangeEvent {
                    range: Some(Range {
                        start: Position {
                            line: 4,
                            character: 0,
                        },
                        end: Position {
                            line: 4,
                            character: 0,
                        },
                    }),
                    range_length: None,
                    text: "  ".to_owned(),
                },
                TextDocumentContentChangeEvent {
                    range: Some(Range {
                        start: Position {
                            line: 4,
                            character: 2,
                        },
                        end: Position {
                            line: 4,
                            character: 2,
                        },
                    }),
                    range_length: None,
                    text: "a".to_owned(),
                },
            ],
        };
        server.did_change(change_params);

        let completion_params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 4,
                    character: 3,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: Some(CompletionContext {
                trigger_kind: CompletionTriggerKind::Invoked,
                trigger_character: None,
            }),
        };
        let response: CompletionResponse = server.completion(completion_params).unwrap();
        let item1 = CompletionItem {
            label: "abc".to_owned(),
            ..CompletionItem::default()
        };
        let item2 = CompletionItem {
            label: "abcd".to_owned(),
            ..CompletionItem::default()
        };
        if let CompletionResponse::List(item) = response {
            assert!(item.items.contains(&item1));
            assert!(item.items.contains(&item2));
        } else {
            assert!(false);
        }
    }
}
