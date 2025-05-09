use std::time::Duration;

use conduwuit::{
	Result, debug, debug_info, debug_warn, error, info, trace, utils::time::parse_timepoint_ago,
};
use conduwuit_service::media::Dim;
use ruma::{
	EventId, Mxc, MxcUri, OwnedMxcUri, OwnedServerName, ServerName,
	events::room::message::RoomMessageEventContent,
};

use crate::{admin_command, utils::parse_local_user_id};

#[admin_command]
pub(super) async fn delete(
	&self,
	mxc: Option<Box<MxcUri>>,
	event_id: Option<Box<EventId>>,
) -> Result<RoomMessageEventContent> {
	if event_id.is_some() && mxc.is_some() {
		return Ok(RoomMessageEventContent::text_plain(
			"Please specify either an MXC or an event ID, not both.",
		));
	}

	if let Some(mxc) = mxc {
		trace!("Got MXC URL: {mxc}");
		self.services
			.media
			.delete(&mxc.as_str().try_into()?)
			.await?;

		return Ok(RoomMessageEventContent::text_plain(
			"Deleted the MXC from our database and on our filesystem.",
		));
	}

	if let Some(event_id) = event_id {
		trace!("Got event ID to delete media from: {event_id}");

		let mut mxc_urls = Vec::with_capacity(4);

		// parsing the PDU for any MXC URLs begins here
		match self.services.rooms.timeline.get_pdu_json(&event_id).await {
			| Ok(event_json) => {
				if let Some(content_key) = event_json.get("content") {
					debug!("Event ID has \"content\".");
					let content_obj = content_key.as_object();

					if let Some(content) = content_obj {
						// 1. attempts to parse the "url" key
						debug!("Attempting to go into \"url\" key for main media file");
						if let Some(url) = content.get("url") {
							debug!("Got a URL in the event ID {event_id}: {url}");

							if url.to_string().starts_with("\"mxc://") {
								debug!("Pushing URL {url} to list of MXCs to delete");
								let final_url = url.to_string().replace('"', "");
								mxc_urls.push(final_url);
							} else {
								info!(
									"Found a URL in the event ID {event_id} but did not start \
									 with mxc://, ignoring"
								);
							}
						}

						// 2. attempts to parse the "info" key
						debug!("Attempting to go into \"info\" key for thumbnails");
						if let Some(info_key) = content.get("info") {
							debug!("Event ID has \"info\".");
							let info_obj = info_key.as_object();

							if let Some(info) = info_obj {
								if let Some(thumbnail_url) = info.get("thumbnail_url") {
									debug!("Found a thumbnail_url in info key: {thumbnail_url}");

									if thumbnail_url.to_string().starts_with("\"mxc://") {
										debug!(
											"Pushing thumbnail URL {thumbnail_url} to list of \
											 MXCs to delete"
										);
										let final_thumbnail_url =
											thumbnail_url.to_string().replace('"', "");
										mxc_urls.push(final_thumbnail_url);
									} else {
										info!(
											"Found a thumbnail URL in the event ID {event_id} \
											 but did not start with mxc://, ignoring"
										);
									}
								} else {
									info!(
										"No \"thumbnail_url\" key in \"info\" key, assuming no \
										 thumbnails."
									);
								}
							}
						}

						// 3. attempts to parse the "file" key
						debug!("Attempting to go into \"file\" key");
						if let Some(file_key) = content.get("file") {
							debug!("Event ID has \"file\".");
							let file_obj = file_key.as_object();

							if let Some(file) = file_obj {
								if let Some(url) = file.get("url") {
									debug!("Found url in file key: {url}");

									if url.to_string().starts_with("\"mxc://") {
										debug!("Pushing URL {url} to list of MXCs to delete");
										let final_url = url.to_string().replace('"', "");
										mxc_urls.push(final_url);
									} else {
										info!(
											"Found a URL in the event ID {event_id} but did not \
											 start with mxc://, ignoring"
										);
									}
								} else {
									info!("No \"url\" key in \"file\" key.");
								}
							}
						}
					} else {
						return Ok(RoomMessageEventContent::text_plain(
							"Event ID does not have a \"content\" key or failed parsing the \
							 event ID JSON.",
						));
					}
				} else {
					return Ok(RoomMessageEventContent::text_plain(
						"Event ID does not have a \"content\" key, this is not a message or an \
						 event type that contains media.",
					));
				}
			},
			| _ => {
				return Ok(RoomMessageEventContent::text_plain(
					"Event ID does not exist or is not known to us.",
				));
			},
		}

		if mxc_urls.is_empty() {
			info!("Parsed event ID {event_id} but did not contain any MXC URLs.");
			return Ok(RoomMessageEventContent::text_plain(
				"Parsed event ID but found no MXC URLs.",
			));
		}

		let mut mxc_deletion_count: usize = 0;

		for mxc_url in mxc_urls {
			match self
				.services
				.media
				.delete(&mxc_url.as_str().try_into()?)
				.await
			{
				| Ok(()) => {
					debug_info!("Successfully deleted {mxc_url} from filesystem and database");
					mxc_deletion_count = mxc_deletion_count.saturating_add(1);
				},
				| Err(e) => {
					debug_warn!("Failed to delete {mxc_url}, ignoring error and skipping: {e}");
					continue;
				},
			}
		}

		return Ok(RoomMessageEventContent::text_plain(format!(
			"Deleted {mxc_deletion_count} total MXCs from our database and the filesystem from \
			 event ID {event_id}."
		)));
	}

	Ok(RoomMessageEventContent::text_plain(
		"Please specify either an MXC using --mxc or an event ID using --event-id of the \
		 message containing an image. See --help for details.",
	))
}

#[admin_command]
pub(super) async fn delete_list(&self) -> Result<RoomMessageEventContent> {
	if self.body.len() < 2
		|| !self.body[0].trim().starts_with("```")
		|| self.body.last().unwrap_or(&"").trim() != "```"
	{
		return Ok(RoomMessageEventContent::text_plain(
			"Expected code block in command body. Add --help for details.",
		));
	}

	let mut failed_parsed_mxcs: usize = 0;

	let mxc_list = self
		.body
		.to_vec()
		.drain(1..self.body.len().checked_sub(1).unwrap())
		.filter_map(|mxc_s| {
			mxc_s
				.try_into()
				.inspect_err(|e| {
					debug_warn!("Failed to parse user-provided MXC URI: {e}");

					failed_parsed_mxcs = failed_parsed_mxcs.saturating_add(1);
				})
				.ok()
		})
		.collect::<Vec<Mxc<'_>>>();

	let mut mxc_deletion_count: usize = 0;

	for mxc in &mxc_list {
		trace!(%failed_parsed_mxcs, %mxc_deletion_count, "Deleting MXC {mxc} in bulk");
		match self.services.media.delete(mxc).await {
			| Ok(()) => {
				debug_info!("Successfully deleted {mxc} from filesystem and database");
				mxc_deletion_count = mxc_deletion_count.saturating_add(1);
			},
			| Err(e) => {
				debug_warn!("Failed to delete {mxc}, ignoring error and skipping: {e}");
				continue;
			},
		}
	}

	Ok(RoomMessageEventContent::text_plain(format!(
		"Finished bulk MXC deletion, deleted {mxc_deletion_count} total MXCs from our database \
		 and the filesystem. {failed_parsed_mxcs} MXCs failed to be parsed from the database.",
	)))
}

#[admin_command]
pub(super) async fn delete_past_remote_media(
	&self,
	duration: String,
	before: bool,
	after: bool,
	yes_i_want_to_delete_local_media: bool,
) -> Result<RoomMessageEventContent> {
	if before && after {
		return Ok(RoomMessageEventContent::text_plain(
			"Please only pick one argument, --before or --after.",
		));
	}
	assert!(!(before && after), "--before and --after should not be specified together");

	let duration = parse_timepoint_ago(&duration)?;
	let deleted_count = self
		.services
		.media
		.delete_all_remote_media_at_after_time(
			duration,
			before,
			after,
			yes_i_want_to_delete_local_media,
		)
		.await?;

	Ok(RoomMessageEventContent::text_plain(format!(
		"Deleted {deleted_count} total files.",
	)))
}

#[admin_command]
pub(super) async fn delete_all_from_user(
	&self,
	username: String,
) -> Result<RoomMessageEventContent> {
	let user_id = parse_local_user_id(self.services, &username)?;

	let deleted_count = self.services.media.delete_from_user(&user_id).await?;

	Ok(RoomMessageEventContent::text_plain(format!(
		"Deleted {deleted_count} total files.",
	)))
}

#[admin_command]
pub(super) async fn delete_all_from_server(
	&self,
	server_name: Box<ServerName>,
	yes_i_want_to_delete_local_media: bool,
) -> Result<RoomMessageEventContent> {
	if server_name == self.services.globals.server_name() && !yes_i_want_to_delete_local_media {
		return Ok(RoomMessageEventContent::text_plain(
			"This command only works for remote media by default.",
		));
	}

	let Ok(all_mxcs) = self
		.services
		.media
		.get_all_mxcs()
		.await
		.inspect_err(|e| error!("Failed to get MXC URIs from our database: {e}"))
	else {
		return Ok(RoomMessageEventContent::text_plain(
			"Failed to get MXC URIs from our database",
		));
	};

	let mut deleted_count: usize = 0;

	for mxc in all_mxcs {
		let Ok(mxc_server_name) = mxc.server_name().inspect_err(|e| {
			debug_warn!(
				"Failed to parse MXC {mxc} server name from database, ignoring error and \
				 skipping: {e}"
			);
		}) else {
			continue;
		};

		if mxc_server_name != server_name
			|| (self.services.globals.server_is_ours(mxc_server_name)
				&& !yes_i_want_to_delete_local_media)
		{
			trace!("skipping MXC URI {mxc}");
			continue;
		}

		let mxc: Mxc<'_> = mxc.as_str().try_into()?;

		match self.services.media.delete(&mxc).await {
			| Ok(()) => {
				deleted_count = deleted_count.saturating_add(1);
			},
			| Err(e) => {
				debug_warn!("Failed to delete {mxc}, ignoring error and skipping: {e}");
				continue;
			},
		}
	}

	Ok(RoomMessageEventContent::text_plain(format!(
		"Deleted {deleted_count} total files.",
	)))
}

#[admin_command]
pub(super) async fn get_file_info(&self, mxc: OwnedMxcUri) -> Result<RoomMessageEventContent> {
	let mxc: Mxc<'_> = mxc.as_str().try_into()?;
	let metadata = self.services.media.get_metadata(&mxc).await;

	Ok(RoomMessageEventContent::notice_markdown(format!("```\n{metadata:#?}\n```")))
}

#[admin_command]
pub(super) async fn get_remote_file(
	&self,
	mxc: OwnedMxcUri,
	server: Option<OwnedServerName>,
	timeout: u32,
) -> Result<RoomMessageEventContent> {
	let mxc: Mxc<'_> = mxc.as_str().try_into()?;
	let timeout = Duration::from_millis(timeout.into());
	let mut result = self
		.services
		.media
		.fetch_remote_content(&mxc, None, server.as_deref(), timeout)
		.await?;

	// Grab the length of the content before clearing it to not flood the output
	let len = result.content.as_ref().expect("content").len();
	result.content.as_mut().expect("content").clear();

	let out = format!("```\n{result:#?}\nreceived {len} bytes for file content.\n```");
	Ok(RoomMessageEventContent::notice_markdown(out))
}

#[admin_command]
pub(super) async fn get_remote_thumbnail(
	&self,
	mxc: OwnedMxcUri,
	server: Option<OwnedServerName>,
	timeout: u32,
	width: u32,
	height: u32,
) -> Result<RoomMessageEventContent> {
	let mxc: Mxc<'_> = mxc.as_str().try_into()?;
	let timeout = Duration::from_millis(timeout.into());
	let dim = Dim::new(width, height, None);
	let mut result = self
		.services
		.media
		.fetch_remote_thumbnail(&mxc, None, server.as_deref(), timeout, &dim)
		.await?;

	// Grab the length of the content before clearing it to not flood the output
	let len = result.content.as_ref().expect("content").len();
	result.content.as_mut().expect("content").clear();

	let out = format!("```\n{result:#?}\nreceived {len} bytes for file content.\n```");
	Ok(RoomMessageEventContent::notice_markdown(out))
}
