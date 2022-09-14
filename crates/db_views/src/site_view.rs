use crate::structs::SiteView;
use diesel::{result::Error, *};
use lemmy_db_schema::{
  aggregates::structs::SiteAggregates,
  schema::{site, site_aggregates, language, site_language},
  source::site::Site,
};use lemmy_db_schema::source::language::Language;

impl SiteView {
  pub fn read_local(conn: &PgConnection) -> Result<Self, Error> {
    conn.build_transaction().read_write().run(|| {
      let (mut site, counts) = site::table
          .inner_join(site_aggregates::table)
          .select((site::all_columns, site_aggregates::all_columns))
          .order_by(site::id)
          .first::<(Site, SiteAggregates)>(conn)?;

      let languages = site_language::table
          .inner_join(site::table)
          .select(language::all_columns)
          .filter(site::id.eq(site.id))
          .load::<Language>(conn)?;

      site.private_key = None;
      Ok(SiteView { site, counts, languages })
    })
  }
}
