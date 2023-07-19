# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.6.0-rc.3] - 2023-07-19

### Added

- Add proxy option

## [0.6.0-rc.2] - 2023-07-14

### Breaking

- Inn mod could set limit edit seconds(set 0 to disable)

### Fixed

- Atom feed content from summary if its content is empty.

## [0.6.0-rc.1] - 2023-07-07

### Breaking

- The old version of the database is not compatible with the new version. Please backup the database first. If you want to keep old data, please see [examples/disable_compress.rs](./examples/disable_compress.rs) for data migration.

## [0.5.1] - 2023-07-07

### Breaking

- Site admin could set global default page(but it Will make your site config broken, please backup site info first)

### Fixed
- Inn feed missing bug
- unsubscribe button
- better not found error page
- Fix broken comment anchor [8ea5f8f](https://github.com/freedit-org/freedit/commit/8ea5f8f8796ac8119ecf8e0220f178bcb17ac312)
- Add robots.txt [3a5051f](https://github.com/freedit-org/freedit/commit/3a5051fdd6159bef711dc455b858e8171dcecaf6)
- Fix all broken link [4632907](https://github.com/freedit-org/freedit/commit/4632907680bcff58bf39b55a3dcbb8e372a5599a)
- code highlight fixed
- #106
- use br instead of gzip
- inn feed missing bug

## [0.5.0] - 2023-06-21

It's not a breaking change, but it requires more memory for tantivy. If you don't want to upgrade, you can use the old version.

### Added

- Search with tantivy :tada: :tada: :tada:
- add the @user function when posting
- support /inn/inn_name && /user/username
- move explore tab to the side
- Display tips for inn feed posts and old posts
- feed read tree view 

### Fixed
- update crates
- fix home page resetting bug
- remove `State<Db>`
- remove search bar in search page 

## [0.4.5] - 2023-05-19

### Fixed

- fix #47 
- add help link

## [0.4.4] - 2023-05-19

### Fixed

- truncate feed read title 
- remove unnecessary error log

## [0.4.3] - 2023-05-17

### Fixed

- <https://freedit.eu/post/1/75>
- fix #109
- <https://freedit.eu/post/1/73>

## [0.4.2] - 2023-05-11

### Fixed

- fix #109
- fix #107
- set default cookie Max-Age 4h to avoid panic
- inn mod code 8 ->7 
-  update timeline if comment has been deleted 

## [0.4.1] - 2023-04-14

- upgrade crates
- fix https://freedit.eu/post/1/52

## [0.4.0] - 2023-03-31

**breaking changes**

- upgrade crate [bincode](https://github.com/bincode-org/bincode/releases/tag/v2.0.0-rc.3) to rc.3, which breaks enum with value. Make sure backup first and run [examples/post](./examples/post.rs) for data migration.

## [0.3.7] - 2023-03-30

### Fixed

- fix <https://freedit.eu/post/1/45?anchor=0&is_desc=false#4> if you upgrade from v0.3.6, run [example/name](https://github.com/freedit-org/freedit/blob/main/examples/name.rs) first.

## [0.3.6] - 2023-03-29

### Fixed

- fix #97
- fix #88
- fix <https://freedit.eu/post/1/47>

## [0.3.5] - 2023-03-28

### Fixed

- fix <https://freedit.eu/post/1/45>

## [0.3.4] - 2023-03-17

### Fixed

- allow comment only if normal status

## [0.3.3] - 2023-03-17

### Fixed

- panic bug: visibility error
- fix #89 (reported by @Yakumo-Yukari)
- fix https://freedit.eu/post/1/38?#1 (reported by @Alice)

## [0.3.1] - 2023-03-06

### Fixed

- Changed cookie name from `__Host-id` to `id`, fix #86 (Reported by @dominikdalek )
- Fixed #85 (Reported by @dominikdalek )

## [0.3.0] - 2023-03-02

**breaking changes**

- `Post` add field `status`, remove field `is_locked` and `is_hidden`
- `Post` field `content` changed to `PostContent`
- tree `user_uploads`: changed from `uid#image_hash.ext => &[]` to `uid#img_id => image_hash.ext`
- rewrite notifications: tree "notifications" changed from old kv: `uid#pid#cid => notification_code` to new kv: `uid#nid#nt_type => id1#id2#is_read`

### Added

- author can delete post if no one comments it
- `/gallery` Fix #64
- Auto post from inn feed.

### Fixed

- if the comment has been deleted, just remove it 
- Table style missing #42
- username could not contain special characters (#77 reported by @Yakumo-Yukari)
- Feed update timeouts should be less than global timeouts
- remove notification if the msg is deleted #67 
- Solo like should be descending fix #68
- /user/list filter is broken #69

## [0.2.10] - 2023-02-02

### Fixed

- fullscreen background (by [thomas992](https://github.com/thomas992) [#61](https://github.com/freedit-org/freedit/pull/61))
- push footer to the bottom of the page (by [pleshevskiy](https://github.com/pleshevskiy) [#66](https://github.com/freedit-org/freedit/pull/66)) 

- csp: allow imgs from subdomain

## [0.2.9] - 2023-01-31

### Added

- Add git commit hash
- default checked for draft

## [0.2.8] - 2023-01-17

### Added

- Save as draft

## [0.2.7] - 2023-01-17

### Added

- Show errors if updating feed unsuccessfully

## [0.2.6] - 2022-12-30

Happy new year! 🎉🎉🎉

### Fixed

- panic bug fixed: get inn list by topic
- bug fixed: remove duplicated tags and topics
- fixed: don't update timestamp when edit post
- No joined inn found, return err

### Added

- New post button

### Changed 

- Update crates
- Cargo clippy beta
- Refresh feeds asynchronously
- Stop browser requesting favicon

## [0.2.5] - 2022-12-09

### Changed 

- Changed svgs to independent files
- Update crates
- Add rss reader feature in readme

### Fixed

- Fixed inn page members number display error
- Feed unread/star pagination error

### Added

- Add [CHANGELOG.md](./CHANGELOG.md)

## [0.2.4] - 2022-12-01

[unreleased]: https://github.com/freedit-org/freedit/compare/v0.5.1...HEAD
[0.5.1]: https://github.com/freedit-org/freedit/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/freedit-org/freedit/compare/v0.4.5...v0.5.0
[0.4.5]: https://github.com/freedit-org/freedit/compare/v0.4.4...v0.4.5
[0.4.4]: https://github.com/freedit-org/freedit/compare/v0.4.3...v0.4.4
[0.4.3]: https://github.com/freedit-org/freedit/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/freedit-org/freedit/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/freedit-org/freedit/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/freedit-org/freedit/compare/v0.3.7...v0.4.0
[0.3.7]: https://github.com/freedit-org/freedit/compare/v0.3.6...v0.3.7
[0.3.6]: https://github.com/freedit-org/freedit/compare/v0.3.5...v0.3.6
[0.3.5]: https://github.com/freedit-org/freedit/compare/v0.3.4...v0.3.5
[0.3.4]: https://github.com/freedit-org/freedit/compare/v0.3.3...v0.3.4
[0.3.3]: https://github.com/freedit-org/freedit/compare/v0.3.1...v0.3.3
[0.3.1]: https://github.com/freedit-org/freedit/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/freedit-org/freedit/compare/v0.2.10...v0.3.0
[0.2.10]: https://github.com/freedit-org/freedit/compare/v0.2.9...v0.2.10
[0.2.9]: https://github.com/freedit-org/freedit/compare/v0.2.8...v0.2.9
[0.2.8]: https://github.com/freedit-org/freedit/compare/v0.2.7...v0.2.8
[0.2.7]: https://github.com/freedit-org/freedit/compare/v0.2.6...v0.2.7
[0.2.6]: https://github.com/freedit-org/freedit/compare/v0.2.5...v0.2.6
[0.2.5]: https://github.com/freedit-org/freedit/compare/v0.2.4...v0.2.5
[0.2.4]: https://github.com/freedit-org/freedit/compare/v0.2.3...v0.2.4
[0.2.3]: https://github.com/freedit-org/freedit/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/freedit-org/freedit/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/freedit-org/freedit/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/freedit-org/freedit/compare/v0.1.4...v0.2.0
[0.1.4]: https://github.com/freedit-org/freedit/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/freedit-org/freedit/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/freedit-org/freedit/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/freedit-org/freedit/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/freedit-org/freedit/releases/tag/v0.1.0