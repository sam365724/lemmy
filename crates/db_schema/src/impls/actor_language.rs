use crate::{
  newtypes::{CommunityId, LanguageId, LocalUserId, SiteId},
  source::{actor_language::*, community::Community, language::Language, site::Site},
};
use diesel::{
  delete,
  dsl::*,
  insert_into,
  result::Error,
  select,
  ExpressionMethods,
  PgConnection,
  QueryDsl,
  RunQueryDsl,
};
use lemmy_utils::error::LemmyError;

impl LocalUserLanguage {
  pub fn read(
    conn: &PgConnection,
    for_local_user_id: LocalUserId,
  ) -> Result<Vec<LanguageId>, Error> {
    use crate::schema::local_user_language::dsl::*;

    local_user_language
      .filter(local_user_id.eq(for_local_user_id))
      .select(language_id)
      .get_results(conn)
  }

  /// Update the user's languages.
  ///
  /// If no language_id vector is given, it will show all languages
  pub fn update(
    conn: &PgConnection,
    language_ids: Vec<LanguageId>,
    for_local_user_id: LocalUserId,
  ) -> Result<(), Error> {
    conn.build_transaction().read_write().run(|| {
      use crate::schema::local_user_language::dsl::*;
      // Clear the current user languages
      delete(local_user_language.filter(local_user_id.eq(for_local_user_id))).execute(conn)?;

      let lang_ids = update_languages(conn, language_ids)?;
      for l in lang_ids {
        let form = LocalUserLanguageForm {
          local_user_id: for_local_user_id,
          language_id: l,
        };
        insert_into(local_user_language)
          .values(form)
          .get_result::<Self>(conn)?;
      }
      Ok(())
    })
  }
}

impl SiteLanguage {
  pub fn read_local(conn: &PgConnection) -> Result<Vec<LanguageId>, Error> {
    conn.build_transaction().read_write().run(|| {
      let local_site = Site::read_local(conn)?;
      SiteLanguage::read(conn, local_site.id)
    })
  }

  pub fn read(conn: &PgConnection, for_site_id: SiteId) -> Result<Vec<LanguageId>, Error> {
    use crate::schema::site_language::dsl::*;
    site_language
      .filter(site_id.eq(for_site_id))
      .select(language_id)
      .load(conn)
  }

  pub fn update(
    conn: &PgConnection,
    language_ids: Vec<LanguageId>,
    for_site_id: SiteId,
  ) -> Result<(), Error> {
    conn.build_transaction().read_write().run(|| {
      use crate::schema::site_language::dsl::*;
      // Clear the current languages
      delete(site_language.filter(site_id.eq(for_site_id))).execute(conn)?;

      let lang_ids = update_languages(conn, language_ids)?;
      for l in lang_ids.clone() {
        let form = SiteLanguageForm {
          site_id: for_site_id,
          language_id: l,
        };
        insert_into(site_language)
          .values(form)
          .get_result::<Self>(conn)?;
      }

      CommunityLanguage::limit_languages(conn, lang_ids)?;

      Ok(())
    })
  }
}

impl CommunityLanguage {
  /// Returns true if the given language is one of configured languages for given community
  pub fn is_allowed_community_language(
    conn: &PgConnection,
    for_language_id: LanguageId,
    for_community_id: CommunityId,
  ) -> Result<(), LemmyError> {
    use crate::schema::community_language::dsl::*;
    let is_allowed = select(exists(
      community_language
        .filter(language_id.eq(for_language_id))
        .filter(community_id.eq(for_community_id)),
    ))
    .get_result(conn)?;

    if is_allowed {
      Ok(())
    } else {
      Err(LemmyError::from_message("language_not_allowed"))
    }
  }

  /// When site languages are updated, delete all languages of local communities which are not
  /// also part of site languages. This is because post/comment language is only checked against
  /// community language, and it shouldnt be possible to post content in languages which are not
  /// allowed by local site.
  fn limit_languages(conn: &PgConnection, site_language_ids: Vec<LanguageId>) -> Result<(), Error> {
    // this could be implemented using join + delete, but its not supported in diesel yet
    // https://github.com/diesel-rs/diesel/issues/1478

    use crate::schema::{community::dsl::*, community_language::dsl::*};
    let local_communities: Vec<Community> = community.filter(local.eq(true)).load(conn)?;

    for c in local_communities {
      delete(
        community_language
          .filter(community_id.eq(c.id))
          .filter(language_id.ne(all(&site_language_ids))),
      )
      .execute(conn)?;
    }
    Ok(())
  }

  pub fn read(
    conn: &PgConnection,
    for_community_id: CommunityId,
  ) -> Result<Vec<LanguageId>, Error> {
    use crate::schema::community_language::dsl::*;
    community_language
      .filter(community_id.eq(for_community_id))
      .select(language_id)
      .load(conn)
  }

  pub fn update(
    conn: &PgConnection,
    language_ids: Vec<LanguageId>,
    for_community_id: CommunityId,
  ) -> Result<(), Error> {
    conn.build_transaction().read_write().run(|| {
      use crate::schema::community_language::dsl::*;
      // Clear the current languages
      delete(community_language.filter(community_id.eq(for_community_id))).execute(conn)?;

      let lang_ids = update_languages(conn, language_ids)?;
      for l in lang_ids {
        let form = CommunityLanguageForm {
          community_id: for_community_id,
          language_id: l,
        };
        insert_into(community_language)
          .values(form)
          .get_result::<Self>(conn)?;
      }
      Ok(())
    })
  }
}

// If no language is given, set all languages
fn update_languages(
  conn: &PgConnection,
  language_ids: Vec<LanguageId>,
) -> Result<Vec<LanguageId>, Error> {
  if language_ids.is_empty() {
    Ok(
      Language::read_all(conn)?
        .into_iter()
        .map(|l| l.id)
        .collect(),
    )
  } else {
    Ok(language_ids)
  }
}

#[cfg(test)]
mod tests {
  use crate::{
    impls::actor_language::*,
    source::{
      community::CommunityForm,
      local_user::{LocalUser, LocalUserForm},
      person::{Person, PersonForm},
      site::SiteForm,
    },
    traits::Crud,
    utils::establish_unpooled_connection,
  };
  use serial_test::serial;

  fn test_langs1(conn: &PgConnection) -> Vec<LanguageId> {
    vec![
      Language::read_id_from_code(conn, "en").unwrap(),
      Language::read_id_from_code(conn, "fr").unwrap(),
      Language::read_id_from_code(conn, "ru").unwrap(),
    ]
  }
  fn test_langs2(conn: &PgConnection) -> Vec<LanguageId> {
    vec![
      Language::read_id_from_code(conn, "fi").unwrap(),
      Language::read_id_from_code(conn, "se").unwrap(),
    ]
  }

  fn create_test_site(conn: &PgConnection) -> Site {
    let site_form = SiteForm {
      name: "test site".to_string(),
      ..Default::default()
    };
    Site::create(conn, &site_form).unwrap()
  }

  #[test]
  #[serial]
  fn test_update_languages() {
    let conn = establish_unpooled_connection();

    // call with empty vec, returns all languages
    let updated1 = update_languages(&conn, vec![]).unwrap();
    assert_eq!(184, updated1.len());

    // call with nonempty vec, returns same vec
    let test_langs = test_langs1(&conn);
    let updated2 = update_languages(&conn, test_langs.clone()).unwrap();
    assert_eq!(test_langs, updated2);
  }

  #[test]
  #[serial]
  fn test_site_languages() {
    let conn = establish_unpooled_connection();

    let site = create_test_site(&conn);
    let site_languages1 = SiteLanguage::read_local(&conn).unwrap();
    // site is created with all languages
    assert_eq!(184, site_languages1.len());

    let test_langs = test_langs1(&conn);
    SiteLanguage::update(&conn, test_langs.clone(), site.id).unwrap();

    let site_languages2 = SiteLanguage::read_local(&conn).unwrap();
    // after update, site only has new languages
    assert_eq!(test_langs, site_languages2);

    Site::delete(&conn, site.id).unwrap();
  }

  #[test]
  #[serial]
  fn test_user_languages() {
    let conn = establish_unpooled_connection();

    let site = create_test_site(&conn);
    let test_langs = test_langs1(&conn);
    SiteLanguage::update(&conn, test_langs.clone(), site.id).unwrap();

    let person_form = PersonForm {
      name: "my test person".to_string(),
      public_key: Some("pubkey".to_string()),
      ..Default::default()
    };
    let person = Person::create(&conn, &person_form).unwrap();
    let local_user_form = LocalUserForm {
      person_id: Some(person.id),
      password_encrypted: Some("my_pw".to_string()),
      ..Default::default()
    };
    let local_user = LocalUser::create(&conn, &local_user_form).unwrap();
    let local_user_langs1 = LocalUserLanguage::read(&conn, local_user.id).unwrap();

    // new user should be initialized with site languages
    assert_eq!(test_langs, local_user_langs1);

    // update user languages
    LocalUserLanguage::update(&conn, test_langs2(&conn), local_user.id).unwrap();
    let local_user_langs2 = LocalUserLanguage::read(&conn, local_user.id).unwrap();
    assert_eq!(2, local_user_langs2.len());

    Person::delete(&conn, person.id).unwrap();
    LocalUser::delete(&conn, local_user.id).unwrap();
    Site::delete(&conn, site.id).unwrap();
  }

  #[test]
  #[serial]
  fn test_community_languages() {
    let conn = establish_unpooled_connection();
    let site = create_test_site(&conn);
    let test_langs = test_langs1(&conn);
    SiteLanguage::update(&conn, test_langs.clone(), site.id).unwrap();

    let community_form = CommunityForm {
      name: "test community".to_string(),
      title: "test community".to_string(),
      public_key: Some("pubkey".to_string()),
      ..Default::default()
    };
    let community = Community::create(&conn, &community_form).unwrap();
    let community_langs1 = CommunityLanguage::read(&conn, community.id).unwrap();
    // community is initialized with site languages
    assert_eq!(test_langs, community_langs1);

    let allowed_lang1 =
      CommunityLanguage::is_allowed_community_language(&conn, test_langs[0], community.id);
    assert!(allowed_lang1.is_ok());

    let test_langs2 = test_langs2(&conn);
    let allowed_lang2 =
      CommunityLanguage::is_allowed_community_language(&conn, test_langs2[0], community.id);
    assert!(allowed_lang2.is_err());

    // limit site languages to en, fi. after this, community languages should be updated to
    // intersection of old languages (en, fr, ru) and (en, fi), which is only fi.
    CommunityLanguage::limit_languages(&conn, vec![test_langs[0], test_langs2[0]]).unwrap();
    let community_langs2 = CommunityLanguage::read(&conn, community.id).unwrap();
    assert_eq!(vec![test_langs[0]], community_langs2);

    // update community languages to different ones
    CommunityLanguage::update(&conn, test_langs2.clone(), community.id).unwrap();
    let community_langs3 = CommunityLanguage::read(&conn, community.id).unwrap();
    assert_eq!(test_langs2, community_langs3);

    Site::delete(&conn, site.id).unwrap();
    Community::delete(&conn, community.id).unwrap();
  }
}
