use anyhow::{Context, Result, ensure};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, BufWriter};

pub(super) async fn portable_dump(
    input: impl AsyncRead + Unpin,
    output: impl AsyncWrite + Unpin,
) -> Result<()> {
    let mut input = BufReader::new(input);
    let mut output = BufWriter::new(output);
    let mut quote = None;
    let mut escaped = false;
    let mut block = false;
    let mut line = false;
    let mut previous = 0;
    let mut word = Vec::new();
    let mut after_create = false;
    while let Some(byte) = next(&mut input).await? {
        output.write_all(&[byte]).await?;
        if line {
            line = byte != b'\n';
        } else if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' && delimiter != b'`' {
                escaped = true;
            } else if byte == delimiter {
                quote = None;
            }
        } else if block {
            if previous == b'*' && byte == b'/' {
                block = false;
            }
        } else if byte == b'#' || (previous == b'-' && byte == b'-') {
            line = true;
        } else if matches!(byte, b'\'' | b'"' | b'`') {
            quote = Some(byte);
        } else if previous == b'/' && byte == b'*' {
            block = true;
            let Some(first) = next(&mut input).await? else {
                break;
            };
            output.write_all(&[first]).await?;
            if first == b'!' {
                let mut prefix = Vec::new();
                while prefix.len() < 32 {
                    let Some(byte) = next(&mut input).await? else {
                        break;
                    };
                    prefix.push(byte);
                    if !byte.is_ascii_digit() && !byte.is_ascii_whitespace() {
                        break;
                    }
                }
                if prefix.last() == Some(&b'D') {
                    for _ in 0..7 {
                        if let Some(byte) = next(&mut input).await? {
                            prefix.push(byte);
                        }
                    }
                    if prefix.ends_with(b"DEFINER=") {
                        output.write_all(&prefix).await?;
                        quoted_account(&mut input).await?;
                        ensure!(
                            next(&mut input).await? == Some(b'@'),
                            "Malformed MySQL dump definer"
                        );
                        quoted_account(&mut input).await?;
                        output.write_all(b"CURRENT_USER").await?;
                    } else {
                        output.write_all(&prefix).await?;
                    }
                } else {
                    output.write_all(&prefix).await?;
                }
            }
            previous = first;
            continue;
        }
        if quote.is_none() && !block && !line {
            if byte.is_ascii_alphabetic() && word.len() < 16 {
                word.push(byte.to_ascii_uppercase());
            } else if !word.is_empty() {
                if after_create && word == b"DEFINER" && byte == b'=' {
                    quoted_account(&mut input).await?;
                    ensure!(
                        next(&mut input).await? == Some(b'@'),
                        "Malformed MySQL dump definer"
                    );
                    quoted_account(&mut input).await?;
                    output.write_all(b"CURRENT_USER").await?;
                    after_create = false;
                } else {
                    after_create = word == b"CREATE";
                }
                word.clear();
            }
        }
        previous = byte;
    }
    output.flush().await?;
    Ok(())
}

async fn quoted_account(input: &mut (impl tokio::io::AsyncBufRead + Unpin)) -> Result<()> {
    let quote = next(input)
        .await?
        .context("Incomplete MySQL dump definer")?;
    ensure!(
        matches!(quote, b'`' | b'\''),
        "Unsupported MySQL dump definer"
    );
    loop {
        let byte = next(input)
            .await?
            .context("Incomplete MySQL dump definer")?;
        if byte == b'\\' && quote != b'`' {
            next(input)
                .await?
                .context("Incomplete MySQL dump definer escape")?;
        } else if byte == quote {
            // Read the following character through a one-byte lookahead in
            // the caller's buffered input, retaining doubled identifier quotes.
            let buffer = tokio::io::AsyncBufReadExt::fill_buf(input).await?;
            if buffer.first() == Some(&quote) {
                tokio::io::AsyncBufReadExt::consume(input, 1);
            } else {
                return Ok(());
            }
        }
    }
}

async fn next(input: &mut (impl AsyncRead + Unpin)) -> Result<Option<u8>> {
    let mut byte = [0];
    Ok((input.read(&mut byte).await? != 0).then_some(byte[0]))
}
