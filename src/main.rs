use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::{env, error::Error, io, time::Duration};
use tokio::sync::mpsc;

const MAX_INPUT_LEN: usize = 500;
const REQUEST_TIMEOUT_SECS: u64 = 20;

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    content: String,
}

/// Application state
struct App {
    input: String,
    result_spl: String,
    is_loading: bool,
}

impl App {
    fn new() -> App {
        App {
            input: String::new(),
            result_spl: String::from(
                "Type your English query above and press Enter. Press Esc to quit.",
            ),
            is_loading: false,
        }
    }
}

struct TerminalCleanup;

impl Drop for TerminalCleanup {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, LeaveAlternateScreen, DisableMouseCapture);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();
    let api_key = env::var("OPENAI_API_KEY").unwrap_or_default();

    // Setup terminal
    enable_raw_mode()?;
    let _cleanup = TerminalCleanup;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Channel for sending API responses back to the UI thread
    let (tx, mut rx) = mpsc::channel::<String>(32);
    let client = Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()?;

    let mut app = App::new();
    if api_key.is_empty() {
        app.result_spl =
            "Missing OPENAI_API_KEY. Add it to your environment or .env file.".to_string();
    }

    loop {
        terminal.draw(|f| ui(f, &app))?;

        // 1. Check for API responses without blocking
        if let Ok(response) = rx.try_recv() {
            app.result_spl = response;
            app.is_loading = false;
        }

        // 2. Poll for keyboard input (timeout ensures we keep looping to check for API responses)
        if crossterm::event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Esc => break, // Quit
                    KeyCode::Char(c) => {
                        if app.input.len() < MAX_INPUT_LEN {
                            app.input.push(c);
                        }
                    }
                    KeyCode::Backspace => {
                        app.input.pop();
                    }
                    KeyCode::Enter => {
                        if api_key.is_empty() {
                            app.result_spl =
                                "Missing OPENAI_API_KEY. Configure it before submitting queries."
                                    .to_string();
                        } else if !app.input.is_empty() && !app.is_loading {
                            app.is_loading = true;
                            app.result_spl = String::from("Translating...");

                            // Spawn a background task for the API call
                            let query = app.input.clone();
                            let api_key_clone = api_key.clone();
                            let client_clone = client.clone();
                            let tx_clone = tx.clone();

                            tokio::spawn(async move {
                                let result = fetch_spl(&client_clone, &api_key_clone, &query).await;
                                let _ = tx_clone.send(result).await;
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    terminal.show_cursor()?;

    Ok(())
}

/// Renders the layout and widgets
fn ui(f: &mut ratatui::Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([Constraint::Length(3), Constraint::Min(1)].as_ref())
        .split(f.size());

    // Input widget
    let input_widget = Paragraph::new(app.input.as_str())
        .style(Style::default().fg(Color::Yellow))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" English Query "),
        );

    f.render_widget(input_widget, chunks[0]);

    // Output widget
    let output_color = if app.is_loading {
        Color::DarkGray
    } else {
        Color::Green
    };
    let output_widget = Paragraph::new(app.result_spl.as_str())
        .style(Style::default().fg(output_color))
        .block(Block::default().borders(Borders::ALL).title(" SPL Result "));

    f.render_widget(output_widget, chunks[1]);

    // Render the cursor manually
    let max_cursor_x = chunks[0].x + chunks[0].width.saturating_sub(2);
    let desired_x = chunks[0]
        .x
        .saturating_add(app.input.chars().count() as u16)
        .saturating_add(1);
    f.set_cursor(desired_x.min(max_cursor_x), chunks[0].y + 1);
}

/// Handles the external API request
async fn fetch_spl(client: &Client, api_key: &str, query: &str) -> String {
    let system_prompt = "You are an expert Splunk administrator. Translate the user's English description into a valid Splunk Processing Language (SPL) query. Output ONLY the raw SPL code.";
    let request_body = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": query}
        ],
        "temperature": 0.0
    });

    let res = client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(api_key)
        .json(&request_body)
        .send()
        .await;

    match res {
        Ok(response) => {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();

            if status.is_success() {
                if let Ok(data) = serde_json::from_str::<ChatResponse>(&body) {
                    if let Some(choice) = data.choices.first() {
                        return choice.message.content.trim().to_string();
                    }
                }
                return format!(
                    "Error: Failed to parse API response ({}): {}",
                    status,
                    truncate_for_display(&body, 300)
                );
            }

            format!("API Error {}: {}", status, truncate_for_display(&body, 300))
        }
        Err(e) => format!("Request failed: {}", e),
    }
}

fn truncate_for_display(input: &str, max_chars: usize) -> String {
    let mut out = input.chars().take(max_chars).collect::<String>();
    if input.chars().count() > max_chars {
        out.push_str("...");
    }
    out
}
