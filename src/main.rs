use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::{env, error::Error, io, time::Duration};
use tokio::sync::mpsc;

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
            result_spl: String::from("Type your English query above and press Enter. Press Esc to quit."),
            is_loading: false,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Channel for sending API responses back to the UI thread
    let (tx, mut rx) = mpsc::channel::<String>(32);
    let client = Client::new();

    let mut app = App::new();

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
                    KeyCode::Char(c) => app.input.push(c),
                    KeyCode::Backspace => {
                        app.input.pop();
                    }
                    KeyCode::Enter => {
                        if !app.input.is_empty() && !app.is_loading {
                            app.is_loading = true;
                            app.result_spl = String::from("Translating...");

                            // Spawn a background task for the API call
                            let query = app.input.clone();
                            let api_key_clone = api_key.clone();
                            let client_clone = client.clone();
                            let tx_clone = tx.clone();

                            tokio::spawn(async move {
                                let result = fetch_spl(client_clone, api_key_clone, query).await;
                                let _ = tx_clone.send(result).await;
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
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
        .block(Block::default().borders(Borders::ALL).title(" English Query "));

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
    f.set_cursor(chunks[0].x + app.input.len() as u16 + 1, chunks[0].y + 1);
}

/// Handles the external API request
async fn fetch_spl(client: Client, api_key: String, query: String) -> String {
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
        Ok(response) if response.status().is_success() => {
            if let Ok(data) = response.json::<ChatResponse>().await {
                if let Some(choice) = data.choices.first() {
                    return choice.message.content.trim().to_string();
                }
            }
            "Error: Failed to parse API response.".to_string()
        }
        Ok(response) => format!("API Error: {}", response.status()),
        Err(e) => format!("Request failed: {}", e),
    }
}
