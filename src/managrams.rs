use log::{debug, error, info};

use reqwest::{blocking::Client, Url};

use anyhow::{Context, Result};

use crate::{
    db, log_if_err,
    manifold::{self, GetManagramsArgs, Managram, SendManagramArgs},
    metaculus, mirror,
    settings::Settings,
};

/// Fetch managrams from manifold and save to db for processing.
pub fn sync_managrams(client: &Client, db: &rusqlite::Connection, config: &Settings) -> Result<()> {
    info!("Syncing managrams");
    let last_managram_timestamp = db::last_managram_timestamp(db)?;
    for managram in manifold::get_managrams_depaginated(
        client,
        GetManagramsArgs {
            to_id: Some(config.manifold.user_id.to_owned()),
            after: last_managram_timestamp,
            ..Default::default()
        },
        config,
    )? {
        debug!("Inserting managram into db: {:?}", managram);
        db::insert_managram(db, &managram)?;
    }

    Ok(())
}

pub fn process_managrams(
    client: &Client,
    db: &rusqlite::Connection,
    config: &Settings,
) -> Result<()> {
    for managram in db::get_unprocessed_managrams(db)? {
        log_if_err!(process_managram(client, db, config, &managram)
            .with_context(|| format!("failed to process managram: {:?}", managram)));
    }
    Ok(())
}

fn process_managram(
    client: &Client,
    db: &rusqlite::Connection,
    config: &Settings,
    managram: &Managram,
) -> Result<()> {
    debug!("Processing managram with txn_id {}", managram.id);
    match managram.message.split_once(' ') {
        Some(("mirror", target)) => {
            process_managram_mirror_request(client, db, config, managram, target)?;
        }
        _ => {
            debug!("Managram does not contain known command. Marking processed.",);
            db::set_managram_processed(db, &managram.id, true)?;
        }
    }
    Ok(())
}

fn process_managram_mirror_request(
    client: &Client,
    db: &rusqlite::Connection,
    config: &Settings,
    managram: &Managram,
    target: &str,
) -> Result<()> {
    debug!("Processing managram mirror request.");
    let cfg = &config.manifold.managrams;
    let mut failure_text = None;
    let min_amount = cfg.min_amount + cfg.mirror_cost;
    // TODO: make this not a monstrosity
    if managram.amount >= min_amount {
        if let Some(metaculus_question_id) = extract_metaculus_id_from_url(target) {
            match metaculus::get_question(client, &metaculus_question_id, config) {
                Ok(metaculus_question) => {
                    if let Some(mirror) = db::get_any_mirror(
                        db,
                        &crate::types::QuestionSource::Metaculus,
                        &metaculus_question.id.to_string(),
                    )? {
                        failure_text = Some(format!(
                            "a mirror already exists at {}",
                            mirror.manifold_url()
                        ));
                    } else {
                        match metaculus::check_question_requirements(
                            &metaculus_question,
                            &config.metaculus.request_filter,
                        ) {
                            Ok(()) => {
                                match mirror::mirror_metaculus_question(
                                    client,
                                    db,
                                    config,
                                    &metaculus_question,
                                ) {
                                    Ok(market) => {
                                        db::set_managram_processed(db, &managram.id, true)?;
                                        manifold::send_managram(
                                            client,
                                            config,
                                            &SendManagramArgs {
                                                amount: cfg.min_amount,
                                                to_ids: vec![managram.from_id.clone()],
                                                message: format!(
                                                    "Success! {}",
                                                    market.manifold_url
                                                ),
                                            },
                                        )?;
                                    }
                                    Err(e) => {
                                        error!("error while cloning from request: {:#}", e);
                                        failure_text = Some("unexpected error".to_owned());
                                    }
                                }
                            }
                            Err(e) => {
                                failure_text = Some(e.to_string());
                            }
                        }
                    }
                }
                Err(_) => {
                    failure_text = Some("failed to fetch question from metaculus".to_owned());
                }
            }
        } else {
            failure_text = Some("failed to parse Metaculus question url".to_owned());
        }
    } else {
        failure_text = Some(format!(
            "please include {} mana in mirror request",
            min_amount
        ))
    }
    if let Some(failure_text) = failure_text {
        // mark processed before actually refunding to prevent theft in case of error
        db::set_managram_processed(db, &managram.id, true)?;
        manifold::send_managram(
            client,
            config,
            &SendManagramArgs {
                amount: managram.amount,
                to_ids: vec![managram.from_id.clone()],
                message: format!("mirror failed: {}", failure_text),
            },
        )?;
    }
    Ok(())
}

fn extract_metaculus_id_from_url(url: &str) -> Option<String> {
    if let Ok(url) = url.parse::<Url>() {
        if let Some(domain) = url.domain() {
            // TODO: there has to be a better way to do this
            if domain == "metaculus.com" || domain == "www.metaculus.com" {
                let segments: Vec<&str> = url.path_segments().unwrap().collect();
                if segments.len() >= 2
                    && segments[0] == "questions"
                    && segments[1].parse::<usize>().is_ok()
                {
                    return Some(segments[1].to_string());
                }
            }
        }
    }
    None
}
