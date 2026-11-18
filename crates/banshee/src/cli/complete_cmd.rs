//! `banshee complete` — debug helper that prints completion items at an offset.

use std::path::PathBuf;

use anyhow::{Result, bail};
use banshee_completion::CompletionRequest;
use clap::Args;
use text_size::TextSize;

use super::io::{self, gather_inputs};
use super::{Cli, exit};

#[derive(Args, Debug)]
pub struct CompleteArgs {
    /// File to complete in. Use `-` or omit to read stdin.
    paths: Vec<PathBuf>,

    /// Byte offset of the cursor.
    #[arg(long)]
    offset: u32,

    /// Override completion.max-items (0 = unlimited).
    #[arg(long)]
    limit: Option<usize>,
}

pub fn run(args: &CompleteArgs, cli: &Cli) -> Result<u8> {
    let config = cli.load_config(&io::discovery_anchor(&args.paths))?;
    let inputs = gather_inputs(&args.paths)?;
    let Some(input) = inputs.into_iter().next() else {
        bail!("no input provided");
    };

    let provider = super::schema::resolve(&config)?;

    let templated = config.templater.render(&input.text);
    let offset = TextSize::from(args.offset);
    let (source, offset) = templated
        .as_ref()
        .map_or((input.text.as_str(), offset), |t| {
            (t.text(), t.rendered_offset(offset))
        });
    let settings = &config.completion;
    let mut request = CompletionRequest::new(source, offset)
        .with_keywords(settings.keywords)
        .with_functions(settings.functions)
        .with_limit(args.limit.unwrap_or(settings.max_items));
    if let Some(p) = &provider {
        request = request.with_schema_provider(p).with_function_provider(p);
    }
    let result = banshee_completion::complete(request);

    if result.items.is_empty() {
        println!("(no completions at offset {})", args.offset);
        return Ok(exit::OK);
    }

    for item in result.items {
        let detail = item.detail.as_deref().unwrap_or("");
        println!("{:<24} {:?}  {}", item.label, item.kind, detail);
    }
    Ok(exit::OK)
}
