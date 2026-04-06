mod generated;

use generated::OrmxClient;
use ormx_runtime::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // This example demonstrates the generated API.
    // To actually run queries, you need a database with the right schema.

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://localhost/ormx_example".into());

    println!("Connecting to {database_url}...");
    let client = OrmxClient::connect(&database_url).await?;

    // ─── CREATE ────────────────────────────────────────────
    let _create_query = client.user().create(generated::user::data::UserCreateInput {
        email: "alice@example.com".into(),
        name: Some("Alice".into()),
        role: Some(generated::Role::Admin),
        id: None,         // auto-generated via uuid()
        created_at: None, // auto-generated via now()
    });

    // ─── FIND MANY with filters + pagination ───────────────
    let _find_query = client
        .user()
        .find_many(generated::user::filter::UserWhereInput {
            role: Some(EnumFilter {
                equals: Some(generated::Role::Admin),
                ..Default::default()
            }),
            ..Default::default()
        })
        .order_by(generated::user::order::UserOrderByInput::CreatedAt(
            SortOrder::Desc,
        ))
        .take(10);

    // ─── FIND UNIQUE by email ──────────────────────────────
    let _unique_query = client.user().find_unique(
        generated::user::filter::UserWhereUniqueInput::Email("alice@example.com".into()),
    );

    // ─── UPDATE ────────────────────────────────────────────
    let _update_query = client.user().update(
        generated::user::filter::UserWhereUniqueInput::Id("some-id".into()),
        generated::user::data::UserUpdateInput {
            name: Some(SetValue::Set(Some("Alice Smith".into()))),
            ..Default::default()
        },
    );

    // ─── DELETE ────────────────────────────────────────────
    let _delete_query = client
        .user()
        .delete(generated::user::filter::UserWhereUniqueInput::Id(
            "some-id".into(),
        ));

    // ─── COUNT ─────────────────────────────────────────────
    let _count_query = client
        .user()
        .count(generated::user::filter::UserWhereInput::default());

    // ─── CREATE a post ─────────────────────────────────────
    let _post_query = client.post().create(generated::post::data::PostCreateInput {
        title: "Hello World".into(),
        content: Some("This is my first post.".into()),
        author_id: "some-user-id".into(),
        published: Some(false),
        status: Some(generated::PostStatus::Draft),
        id: None,
        created_at: None,
    });

    // ─── Complex filter with OR ────────────────────────────
    let _complex_query = client.user().find_many(
        generated::user::filter::UserWhereInput {
            or: Some(vec![
                generated::user::filter::UserWhereInput {
                    email: Some(StringFilter {
                        contains: Some("@acme.com".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                generated::user::filter::UserWhereInput {
                    role: Some(EnumFilter {
                        equals: Some(generated::Role::Admin),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        },
    );

    println!("All queries constructed successfully!");
    println!("(exec() methods will be implemented in the next phase)");

    client.disconnect().await;
    Ok(())
}
