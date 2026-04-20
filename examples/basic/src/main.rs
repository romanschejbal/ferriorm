#![allow(clippy::pedantic)]

mod generated;

use generated::FerriormClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://localhost/ferriorm_example".into());

    println!("Connecting to {database_url}...");
    let client = FerriormClient::connect(&database_url).await?;

    // ─── CREATE a user ─────────────────────────────────────
    let user = client
        .user()
        .create(generated::user::data::UserCreateInput {
            email: "alice@example.com".into(),
            name: Some("Alice".into()),
            role: Some(generated::Role::Admin),
            id: None,
            created_at: None,
        })
        .exec()
        .await?;
    println!("Created user: {} (id={})", user.email, user.id);

    // ─── CREATE a post for the user ────────────────────────
    let post = client
        .post()
        .create(generated::post::data::PostCreateInput {
            title: "Hello World".into(),
            content: Some("My first post!".into()),
            author_id: user.id.clone(),
            published: Some(true),
            status: Some(generated::PostStatus::Published),
            view_count: None,
            id: None,
            created_at: None,
        })
        .exec()
        .await?;
    println!("Created post: {} (id={})", post.title, post.id);

    // ─── FIND MANY with include (batched relation loading) ─
    let users_with_posts = client
        .user()
        .find_many(generated::user::filter::UserWhereInput::default())
        .include(generated::user::UserInclude { posts: true })
        .exec()
        .await?;

    for u in &users_with_posts {
        println!(
            "User: {} has {} posts",
            u.data.email,
            u.posts.as_ref().map_or(0, Vec::len)
        );
    }

    // ─── FIND UNIQUE with include ──────────────────────────
    let found = client
        .user()
        .find_unique(generated::user::filter::UserWhereUniqueInput::Email(
            "alice@example.com".into(),
        ))
        .include(generated::user::UserInclude { posts: true })
        .exec()
        .await?;

    if let Some(u) = &found {
        println!(
            "Found: {} with {:?} posts",
            u.data.email,
            u.posts.as_ref().map(Vec::len)
        );
    }

    // ─── Regular CRUD still works ──────────────────────────
    let count = client
        .user()
        .count(generated::user::filter::UserWhereInput::default())
        .exec()
        .await?;
    println!("Total users: {count}");

    // Cleanup
    client
        .post()
        .delete(generated::post::filter::PostWhereUniqueInput::Id(post.id))
        .exec()
        .await?;
    client
        .user()
        .delete(generated::user::filter::UserWhereUniqueInput::Id(user.id))
        .exec()
        .await?;
    println!("Cleaned up.");

    client.disconnect().await;
    Ok(())
}
