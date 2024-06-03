use crate::debug_info;

const ATTACHMENT: &str = "attachment";
const INLINE: &str = "inline";
const APPLICATION_OCTET_STREAM: &str = "application/octet-stream";
const IMAGE_SVG_XML: &str = "image/svg+xml";

/// as defined by MSC2702
const ALLOWED_INLINE_CONTENT_TYPES: [&str; 26] = [
	"text/css",
	"text/plain",
	"text/csv",
	"application/json",
	"application/ld+json",
	"image/jpeg",
	"image/gif",
	"image/png",
	"image/apng",
	"image/webp",
	"image/avif",
	"video/mp4",
	"video/webm",
	"video/ogg",
	"video/quicktime",
	"audio/mp4",
	"audio/webm",
	"audio/aac",
	"audio/mpeg",
	"audio/ogg",
	"audio/wave",
	"audio/wav",
	"audio/x-wav",
	"audio/x-pn-wav",
	"audio/flac",
	"audio/x-flac",
];

/// Returns a Content-Disposition of `attachment` or `inline`, depending on the
/// *parsed* contents of the file uploaded via format magic keys using `infer`
/// crate (basically libmagic without needing libmagic).
#[must_use]
#[tracing::instrument(skip(buf))]
pub fn content_disposition_type(buf: &[u8], content_type: &Option<String>) -> &'static str {
	let Some(file_type) = infer::get(buf) else {
		debug_info!("Failed to infer the file's contents, assuming attachment for Content-Disposition");
		return ATTACHMENT;
	};

	debug_info!("detected MIME type: {}", file_type.mime_type());

	if ALLOWED_INLINE_CONTENT_TYPES.contains(&file_type.mime_type()) {
		INLINE
	} else {
		ATTACHMENT
	}
}

/// overrides the Content-Type with what we detected
///
/// SVG is special-cased due to the MIME type being classified as `text/xml` but
/// browsers need `image/svg+xml`
#[must_use]
#[tracing::instrument(skip(buf))]
pub fn make_content_type(buf: &[u8], content_type: &Option<String>) -> &'static str {
	let Some(file_type) = infer::get(buf) else {
		debug_info!("Failed to infer the file's contents");
		return APPLICATION_OCTET_STREAM;
	};

	let Some(claimed_content_type) = content_type else {
		return file_type.mime_type();
	};

	if claimed_content_type.contains("svg") && file_type.mime_type().contains("xml") {
		return IMAGE_SVG_XML;
	}

	file_type.mime_type()
}

/// sanitises the file name for the Content-Disposition using
/// `sanitize_filename` crate
#[tracing::instrument]
pub fn sanitise_filename(filename: String) -> String {
	let options = sanitize_filename::Options {
		truncate: false,
		..Default::default()
	};

	sanitize_filename::sanitize_with_options(filename, options)
}

/// creates the final Content-Disposition based on whether the filename exists
/// or not, or if a requested filename was specified (media download with
/// filename)
///
/// if filename exists:
/// `Content-Disposition: attachment/inline; filename=filename.ext`
///
/// else: `Content-Disposition: attachment/inline`
#[tracing::instrument(skip(file))]
pub fn make_content_disposition(
	file: &[u8], content_type: &Option<String>, content_disposition: Option<String>, req_filename: Option<String>,
) -> String {
	let filename: String;

	if let Some(req_filename) = req_filename {
		filename = sanitise_filename(req_filename);
	} else {
		filename = content_disposition.map_or_else(String::new, |content_disposition| {
			let (_, filename) = content_disposition
				.split_once("filename=")
				.unwrap_or(("", ""));

			if filename.is_empty() {
				String::new()
			} else {
				sanitise_filename(filename.to_owned())
			}
		});
	};

	if !filename.is_empty() {
		// Content-Disposition: attachment/inline; filename=filename.ext
		format!("{}; filename={}", content_disposition_type(file, content_type), filename)
	} else {
		// Content-Disposition: attachment/inline
		String::from(content_disposition_type(file, content_type))
	}
}

#[cfg(test)]
mod tests {
	#[test]
	fn string_sanitisation() {
		const SAMPLE: &str =
			"🏳️‍⚧️this\\r\\n įs \r\\n ä \\r\nstrïng 🥴that\n\r ../../../../../../../may be\r\n malicious🏳️‍⚧️";
		const SANITISED: &str = "🏳️‍⚧️thisrn įs n ä rstrïng 🥴that ..............may be malicious🏳️‍⚧️";

		let options = sanitize_filename::Options {
			windows: true,
			truncate: true,
			replacement: "",
		};

		// cargo test -- --nocapture
		println!("{SAMPLE}");
		println!("{}", sanitize_filename::sanitize_with_options(SAMPLE, options.clone()));
		println!("{SAMPLE:?}");
		println!("{:?}", sanitize_filename::sanitize_with_options(SAMPLE, options.clone()));

		assert_eq!(SANITISED, sanitize_filename::sanitize_with_options(SAMPLE, options.clone()));
	}
}
