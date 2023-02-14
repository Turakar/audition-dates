alter table waiting_list
drop constraint waiting_list_email_key,
add constraint waiting_list_date_type_email_key unique (date_type, email);
