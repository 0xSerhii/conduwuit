use std::{
	collections::BTreeSet,
	path::Path,
	sync::{atomic::AtomicU32, Arc},
};

use conduwuit::{debug, debug_warn, implement, info, warn, Result};
use rocksdb::{ColumnFamilyDescriptor, Options};

use super::{
	cf_opts::cf_options, db_opts::db_options, descriptor::Descriptor, repair::repair, Db, Engine,
};
use crate::{or_else, Context};

#[implement(Engine)]
#[tracing::instrument(skip_all)]
pub(crate) async fn open(ctx: Arc<Context>, desc: &[Descriptor]) -> Result<Arc<Self>> {
	let server = &ctx.server;
	let config = &server.config;
	let path = &config.database_path;

	let db_opts = db_options(
		config,
		&ctx.env.lock().expect("environment locked"),
		&ctx.row_cache.lock().expect("row cache locked"),
	)?;

	let cfds = Self::configure_cfds(&ctx, &db_opts, desc)?;
	let num_cfds = cfds.len();
	debug!("Configured {num_cfds} column descriptors...");

	let load_time = std::time::Instant::now();
	if config.rocksdb_repair {
		repair(&db_opts, &config.database_path)?;
	}

	debug!("Opening database...");
	let db = if config.rocksdb_read_only {
		Db::open_cf_descriptors_read_only(&db_opts, path, cfds, false)
	} else if config.rocksdb_secondary {
		Db::open_cf_descriptors_as_secondary(&db_opts, path, path, cfds)
	} else {
		Db::open_cf_descriptors(&db_opts, path, cfds)
	}
	.or_else(or_else)?;

	info!(
		columns = num_cfds,
		sequence = %db.latest_sequence_number(),
		time = ?load_time.elapsed(),
		"Opened database."
	);

	Ok(Arc::new(Self {
		read_only: config.rocksdb_read_only,
		secondary: config.rocksdb_secondary,
		corks: AtomicU32::new(0),
		pool: ctx.pool.clone(),
		db,
		ctx,
	}))
}

#[implement(Engine)]
#[tracing::instrument(name = "configure", skip_all)]
fn configure_cfds(
	ctx: &Arc<Context>,
	db_opts: &Options,
	desc: &[Descriptor],
) -> Result<Vec<ColumnFamilyDescriptor>> {
	let server = &ctx.server;
	let config = &server.config;
	let path = &config.database_path;
	let existing = Self::discover_cfs(path, db_opts);
	debug!(
		"Found {} existing columns; have {} described columns",
		existing.len(),
		desc.len()
	);

	existing
		.iter()
		.filter(|&name| name != "default")
		.filter(|&name| !desc.iter().any(|desc| desc.name == name))
		.for_each(|name| {
			debug_warn!("Found unknown column {name:?} in database which will not be opened.");
		});

	desc.iter()
		.filter(|desc| !existing.contains(desc.name))
		.for_each(|desc| {
			debug!(
				"Creating new column {:?} which was not found in the existing database.",
				desc.name,
			);
		});

	let cfopts: Vec<_> = desc
		.iter()
		.map(|desc| cf_options(ctx, db_opts.clone(), desc))
		.collect::<Result<_>>()?;

	let cfds: Vec<_> = desc
		.iter()
		.map(|desc| desc.name)
		.map(ToOwned::to_owned)
		.zip(cfopts.into_iter())
		.map(|(name, opts)| ColumnFamilyDescriptor::new(name, opts))
		.collect();

	Ok(cfds)
}

#[implement(Engine)]
#[tracing::instrument(name = "discover", skip_all)]
fn discover_cfs(path: &Path, opts: &Options) -> BTreeSet<String> {
	Db::list_cf(opts, path)
		.unwrap_or_default()
		.into_iter()
		.collect::<BTreeSet<_>>()
}
