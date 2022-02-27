create table password_resets (
    id serial primary key,
    admin_id text not null,
    token text not null default gen_random_uuid(),
    created timestamp with time zone default now()
)
