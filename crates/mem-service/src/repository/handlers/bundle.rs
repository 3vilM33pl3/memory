use crate::prelude::*;
use crate::*;

pub(crate) const MEMORY_BUNDLE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug)]
pub(crate) struct LoadedBundle {
    manifest: ProjectMemoryBundleManifest,
    warnings: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct ImportAssessment {
    new_count: usize,
    unchanged_count: usize,
    replacing_count: usize,
}

pub(crate) fn entry_key_for_memory(memory: &MemoryEntryResponse) -> String {
    memory.id.to_string()
}

pub(crate) fn entry_hash(entry: &ProjectMemoryBundleEntry) -> Result<String, ApiError> {
    let bytes = serde_json::to_vec(entry).map_err(|error| ApiError::io(error.into()))?;
    Ok(hex_sha256(&bytes))
}

pub(crate) fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub(crate) fn render_bundle_summary(
    source_project: &str,
    entries: &[ProjectMemoryBundleEntry],
    options: &ProjectMemoryExportOptions,
    warning_count: usize,
) -> String {
    let mut type_counts: HashMap<String, usize> = HashMap::new();
    let mut tag_counts: HashMap<String, usize> = HashMap::new();
    for entry in entries {
        *type_counts
            .entry(entry.memory_type.to_string())
            .or_default() += 1;
        for tag in &entry.tags {
            *tag_counts.entry(tag.clone()).or_default() += 1;
        }
    }
    let mut top_types = type_counts.into_iter().collect::<Vec<_>>();
    top_types.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut top_tags = tag_counts.into_iter().collect::<Vec<_>>();
    top_tags.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let sample = entries
        .iter()
        .take(5)
        .map(|entry| format!("- {}", entry.summary))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "# Memory Bundle: {source_project}\n\n\
        - Memories: {}\n\
        - Include archived: {}\n\
        - Include tags: {}\n\
        - Include relations: {}\n\
        - Include source paths: {}\n\
        - Include git commits: {}\n\
        - Include source excerpts: {}\n\
        - Warnings: {}\n\n\
        ## Top memory types\n{}\n\n\
        ## Top tags\n{}\n\n\
        ## Sample memories\n{}\n",
        entries.len(),
        options.include_archived,
        options.include_tags,
        options.include_relations,
        options.include_source_file_paths,
        options.include_git_commits,
        options.include_source_excerpts,
        warning_count,
        top_types
            .iter()
            .take(5)
            .map(|(name, count)| format!("- {name}: {count}"))
            .collect::<Vec<_>>()
            .join("\n"),
        top_tags
            .iter()
            .take(8)
            .map(|(name, count)| format!("- {name}: {count}"))
            .collect::<Vec<_>>()
            .join("\n"),
        if sample.is_empty() {
            "- No memories selected.".to_string()
        } else {
            sample
        },
    )
}

pub(crate) fn detect_bundle_warnings(
    entries: &[ProjectMemoryBundleEntry],
    options: &ProjectMemoryExportOptions,
) -> Vec<String> {
    let email_re = Regex::new(r"[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}").expect("email regex");
    let token_re =
        Regex::new(r"(sk-[A-Za-z0-9_-]{10,}|ghp_[A-Za-z0-9]{20,}|AIza[0-9A-Za-z_-]{20,})")
            .expect("token regex");
    let path_re = Regex::new(r"(/home/|/Users/|[A-Z]:\\)").expect("path regex");
    let phone_re = Regex::new(r"\+?\d[\d \-]{7,}\d").expect("phone regex");
    let mut warnings = Vec::new();

    for entry in entries {
        if email_re.is_match(&entry.canonical_text)
            || token_re.is_match(&entry.canonical_text)
            || path_re.is_match(&entry.canonical_text)
            || phone_re.is_match(&entry.canonical_text)
        {
            warnings.push(format!(
                "Memory '{}' contains text that looks sensitive; review canonical text before sharing.",
                entry.summary
            ));
        }
        if options.include_source_excerpts {
            for source in &entry.sources {
                if let Some(excerpt) = &source.excerpt
                    && (email_re.is_match(excerpt)
                        || token_re.is_match(excerpt)
                        || path_re.is_match(excerpt)
                        || phone_re.is_match(excerpt))
                {
                    warnings.push(format!(
                        "Memory '{}' includes a source excerpt that looks sensitive.",
                        entry.summary
                    ));
                    break;
                }
            }
        }
    }

    warnings.sort();
    warnings.dedup();
    warnings
}

pub(crate) async fn load_project_bundle_entries(
    pool: &PgPool,
    slug: &str,
    options: &ProjectMemoryExportOptions,
) -> Result<Vec<MemoryEntryResponse>, ApiError> {
    let status_filter = if options.include_archived {
        None
    } else {
        Some("active")
    };
    let memories = fetch_project_memories(pool, slug, status_filter, 10_000, 0)
        .await
        .map_err(ApiError::sql)?;
    let mut entries = Vec::with_capacity(memories.items.len());
    for item in memories.items {
        if let Some(detail) = fetch_memory_entry(pool, item.id)
            .await
            .map_err(ApiError::sql)?
        {
            entries.push(detail);
        }
    }
    Ok(entries)
}

pub(crate) fn build_bundle_manifest(
    slug: &str,
    options: &ProjectMemoryExportOptions,
    memories: &[MemoryEntryResponse],
) -> Result<(ProjectMemoryBundleManifest, Vec<String>), ApiError> {
    let key_map = memories
        .iter()
        .map(|memory| (memory.id, entry_key_for_memory(memory)))
        .collect::<HashMap<_, _>>();
    let mut entries = Vec::with_capacity(memories.len());

    for memory in memories {
        let mut relations = Vec::new();
        if options.include_relations {
            for relation in &memory.related_memories {
                if let Some(target_entry_key) = key_map.get(&relation.memory_id) {
                    relations.push(ProjectMemoryBundleEntryRelation {
                        relation_type: relation.relation_type.clone(),
                        target_entry_key: target_entry_key.clone(),
                    });
                }
            }
        }

        let mut sources = Vec::new();
        if options.include_source_file_paths
            || options.include_git_commits
            || options.include_source_excerpts
        {
            for source in &memory.sources {
                sources.push(ProjectMemoryBundleSource {
                    source_kind: source.source_kind.clone(),
                    file_path: options
                        .include_source_file_paths
                        .then(|| source.file_path.clone())
                        .flatten(),
                    git_commit: options
                        .include_git_commits
                        .then(|| source.git_commit.clone())
                        .flatten(),
                    symbol_name: source.symbol_name.clone(),
                    symbol_kind: source.symbol_kind.clone(),
                    excerpt: options
                        .include_source_excerpts
                        .then(|| source.excerpt.clone())
                        .flatten(),
                });
            }
        }

        entries.push(ProjectMemoryBundleEntry {
            entry_key: entry_key_for_memory(memory),
            canonical_text: memory.canonical_text.clone(),
            summary: memory.summary.clone(),
            memory_type: memory.memory_type.clone(),
            importance: memory.importance,
            confidence: memory.confidence,
            tags: if options.include_tags {
                memory.tags.clone()
            } else {
                Vec::new()
            },
            relations,
            sources,
            created_at: memory.created_at,
            updated_at: memory.updated_at,
        });
    }

    let warnings = detect_bundle_warnings(&entries, options);
    let summary_markdown = render_bundle_summary(slug, &entries, options, warnings.len());
    let bundle_id = format!("{slug}-{}", chrono::Utc::now().format("%Y%m%d%H%M%S"));
    let mut manifest = ProjectMemoryBundleManifest {
        schema_version: MEMORY_BUNDLE_SCHEMA_VERSION,
        bundle_id,
        source_project: slug.to_string(),
        exported_at: chrono::Utc::now(),
        summary_markdown,
        bundle_hash: String::new(),
        options: options.clone(),
        entries,
    };
    let hash_input = serde_json::to_vec(&manifest).map_err(|error| ApiError::io(error.into()))?;
    manifest.bundle_hash = hex_sha256(&hash_input);
    Ok((manifest, warnings))
}

pub(crate) fn build_export_preview(
    manifest: &ProjectMemoryBundleManifest,
    warnings: Vec<String>,
) -> ProjectMemoryBundlePreview {
    ProjectMemoryBundlePreview {
        bundle_id: manifest.bundle_id.clone(),
        source_project: manifest.source_project.clone(),
        exported_at: manifest.exported_at,
        summary_markdown: manifest.summary_markdown.clone(),
        memory_count: manifest.entries.len(),
        relation_count: manifest
            .entries
            .iter()
            .map(|entry| entry.relations.len())
            .sum(),
        warning_count: warnings.len(),
        warnings,
        options: manifest.options.clone(),
    }
}

pub(crate) fn bundle_filename(slug: &str, bundle_id: &str) -> String {
    format!("{slug}-{bundle_id}.mlbundle.zip")
}

pub(crate) fn serialize_bundle_archive(
    manifest: &ProjectMemoryBundleManifest,
) -> Result<Vec<u8>, ApiError> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("manifest.json", options)
        .map_err(|error| ApiError::io(error.into()))?;
    let manifest_json =
        serde_json::to_vec_pretty(manifest).map_err(|error| ApiError::io(error.into()))?;
    std::io::Write::write_all(&mut zip, &manifest_json)
        .map_err(|error| ApiError::io(error.into()))?;
    zip.start_file("SUMMARY.md", options)
        .map_err(|error| ApiError::io(error.into()))?;
    std::io::Write::write_all(&mut zip, manifest.summary_markdown.as_bytes())
        .map_err(|error| ApiError::io(error.into()))?;
    let cursor = zip.finish().map_err(|error| ApiError::io(error.into()))?;
    Ok(cursor.into_inner())
}

pub(crate) fn load_bundle_archive(bytes: &[u8]) -> Result<LoadedBundle, ApiError> {
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = ZipArchive::new(cursor).map_err(|error| ApiError::io(error.into()))?;
    let mut manifest_json = String::new();
    zip.by_name("manifest.json")
        .map_err(|error| ApiError::io(error.into()))?
        .read_to_string(&mut manifest_json)
        .map_err(|error| ApiError::io(error.into()))?;
    let manifest: ProjectMemoryBundleManifest =
        serde_json::from_str(&manifest_json).map_err(|error| ApiError::io(error.into()))?;
    if manifest.schema_version != MEMORY_BUNDLE_SCHEMA_VERSION {
        return Err(ApiError::validation(ValidationError::new(
            "unsupported memory bundle schema version",
        )));
    }
    let mut hashable = manifest.clone();
    let bundle_hash = std::mem::take(&mut hashable.bundle_hash);
    let recalculated =
        hex_sha256(&serde_json::to_vec(&hashable).map_err(|error| ApiError::io(error.into()))?);
    if bundle_hash != recalculated {
        return Err(ApiError::validation(ValidationError::new(
            "memory bundle hash verification failed",
        )));
    }
    let warnings = detect_bundle_warnings(&manifest.entries, &manifest.options);
    Ok(LoadedBundle { manifest, warnings })
}

pub(crate) async fn preview_bundle_import(
    pool: &PgPool,
    target_project: &str,
    bundle: &ProjectMemoryBundleManifest,
    warnings: Vec<String>,
) -> Result<ProjectMemoryImportPreview, ApiError> {
    let target_project_id = sqlx::query("SELECT id FROM projects WHERE slug = $1")
        .bind(target_project)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::sql)?
        .map(|row| row.try_get::<Uuid, _>("id"))
        .transpose()
        .map_err(ApiError::sql)?;

    let mut assessment = ImportAssessment {
        new_count: 0,
        unchanged_count: 0,
        replacing_count: 0,
    };

    if let Some(project_id) = target_project_id {
        for entry in &bundle.entries {
            let existing = sqlx::query(
                r#"
                SELECT entry_hash
                FROM imported_memory_entries
                WHERE target_project_id = $1
                  AND bundle_id = $2
                  AND exported_entry_key = $3
                "#,
            )
            .bind(project_id)
            .bind(&bundle.bundle_id)
            .bind(&entry.entry_key)
            .fetch_optional(pool)
            .await
            .map_err(ApiError::sql)?;
            if let Some(row) = existing {
                let existing_hash: String = row.try_get("entry_hash").map_err(ApiError::sql)?;
                if existing_hash == entry_hash(entry)? {
                    assessment.unchanged_count += 1;
                } else {
                    assessment.replacing_count += 1;
                }
            } else {
                assessment.new_count += 1;
            }
        }
    } else {
        assessment.new_count = bundle.entries.len();
    }

    Ok(ProjectMemoryImportPreview {
        bundle_id: bundle.bundle_id.clone(),
        bundle_hash: bundle.bundle_hash.clone(),
        source_project: bundle.source_project.clone(),
        target_project: target_project.to_string(),
        exported_at: bundle.exported_at,
        summary_markdown: bundle.summary_markdown.clone(),
        memory_count: bundle.entries.len(),
        relation_count: bundle
            .entries
            .iter()
            .map(|entry| entry.relations.len())
            .sum(),
        new_count: assessment.new_count,
        unchanged_count: assessment.unchanged_count,
        replacing_count: assessment.replacing_count,
        warning_count: warnings.len(),
        warnings,
        options: bundle.options.clone(),
    })
}

pub async fn upsert_project_slug(pool: &PgPool, slug: &str) -> Result<Uuid, sqlx::Error> {
    let row = sqlx::query(
        r#"
        INSERT INTO projects (id, slug, name, root_path)
        VALUES (gen_random_uuid(), $1, $1, $1)
        ON CONFLICT (slug) DO UPDATE SET name = EXCLUDED.name
        RETURNING id
        "#,
    )
    .bind(slug)
    .fetch_one(pool)
    .await?;
    row.try_get("id")
}

pub(crate) async fn project_bundle_export_preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(options): Json<ProjectMemoryExportOptions>,
) -> Result<Json<ProjectMemoryBundlePreview>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    let pool = &state.pool()?;
    let memories = load_project_bundle_entries(pool, &slug, &options).await?;
    let (manifest, warnings) = build_bundle_manifest(&slug, &options, &memories)?;
    Ok(Json(build_export_preview(&manifest, warnings)))
}

pub(crate) async fn project_bundle_export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(options): Json<ProjectMemoryExportOptions>,
) -> Result<Response, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    let pool = &state.pool()?;
    let memories = load_project_bundle_entries(pool, &slug, &options).await?;
    let (manifest, _) = build_bundle_manifest(&slug, &options, &memories)?;
    let bytes = serialize_bundle_archive(&manifest)?;
    let filename = bundle_filename(&slug, &manifest.bundle_id);
    notify_project_changed(
        &state,
        slug.clone(),
        None,
        ActivityKind::BundleExport,
        format!("Exported memory bundle {}", manifest.bundle_id),
        Some(ActivityDetails::BundleTransfer {
            bundle_id: manifest.bundle_id.clone(),
            item_count: manifest.entries.len(),
            source_project: Some(slug.clone()),
        }),
    );
    let mut response = Response::new(bytes.into());
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/zip"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .map_err(|error| ApiError::io(error.into()))?,
    );
    Ok(response)
}

pub(crate) async fn project_bundle_import_preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    body: Bytes,
) -> Result<Json<ProjectMemoryImportPreview>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    let loaded = load_bundle_archive(&body)?;
    let preview =
        preview_bundle_import(&state.pool()?, &slug, &loaded.manifest, loaded.warnings).await?;
    Ok(Json(preview))
}

pub(crate) async fn project_bundle_import(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    body: Bytes,
) -> Result<Json<ProjectMemoryImportResponse>, ApiError> {
    require_token(&headers, &state.api_token, &state.config.service.bind_addr)?;
    let loaded = load_bundle_archive(&body)?;
    let pool = &state.pool()?;
    let target_project_id = upsert_project_slug(pool, &slug)
        .await
        .map_err(ApiError::sql)?;
    let import_id = Uuid::new_v4();

    sqlx::query(
        r#"
        INSERT INTO memory_bundle_imports (id, target_project_id, bundle_id, bundle_hash, source_project_slug, summary, options_json, imported_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, now())
        "#,
    )
    .bind(import_id)
    .bind(target_project_id)
    .bind(&loaded.manifest.bundle_id)
    .bind(&loaded.manifest.bundle_hash)
    .bind(&loaded.manifest.source_project)
    .bind(&loaded.manifest.summary_markdown)
    .bind(sqlx::types::Json(&loaded.manifest.options))
    .execute(pool)
    .await
    .map_err(ApiError::sql)?;

    let mut imported_ids = Vec::new();
    let mut current_ids = HashMap::new();
    let mut skipped_count = 0usize;
    let mut replaced_count = 0usize;
    let mut imported_count = 0usize;

    for entry in &loaded.manifest.entries {
        let hash = entry_hash(entry)?;
        let existing = sqlx::query(
            r#"
            SELECT memory_entry_id, entry_hash
            FROM imported_memory_entries
            WHERE target_project_id = $1
              AND bundle_id = $2
              AND exported_entry_key = $3
            "#,
        )
        .bind(target_project_id)
        .bind(&loaded.manifest.bundle_id)
        .bind(&entry.entry_key)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::sql)?;

        let mut superseded_memory_id = None;
        if let Some(row) = existing {
            let existing_memory_id: Uuid = row.try_get("memory_entry_id").map_err(ApiError::sql)?;
            let existing_hash: String = row.try_get("entry_hash").map_err(ApiError::sql)?;
            if existing_hash == hash {
                current_ids.insert(entry.entry_key.clone(), existing_memory_id);
                skipped_count += 1;
                continue;
            }
            superseded_memory_id = Some(existing_memory_id);
            replaced_count += 1;
        }

        let memory_id = Uuid::new_v4();
        let (canonical_id, version_no) = if let Some(existing_memory_id) = superseded_memory_id {
            let row = sqlx::query(
                r#"
                SELECT canonical_id, MAX(version_no) OVER (PARTITION BY canonical_id) AS latest
                FROM memory_entries
                WHERE id = $1
                "#,
            )
            .bind(existing_memory_id)
            .fetch_one(pool)
            .await
            .map_err(ApiError::sql)?;
            (
                row.try_get::<Uuid, _>("canonical_id")
                    .map_err(ApiError::sql)?,
                row.try_get::<i32, _>("latest").map_err(ApiError::sql)? + 1,
            )
        } else {
            (memory_id, 1)
        };
        sqlx::query(
            r#"
            INSERT INTO memory_entries
                (id, project_id, canonical_id, version_no, is_tombstone, canonical_text, summary, memory_type, scope, importance, confidence, status, created_at, updated_at, archived_at, search_document)
            VALUES
                ($1, $2, $3, $4, FALSE, $5, $6, $7, 'project', $8, $9, 'active', $10, $11, NULL, to_tsvector('english', $5 || ' ' || $6))
            "#,
        )
        .bind(memory_id)
        .bind(target_project_id)
        .bind(canonical_id)
        .bind(version_no)
        .bind(&entry.canonical_text)
        .bind(&entry.summary)
        .bind(entry.memory_type.to_string())
        .bind(entry.importance)
        .bind(entry.confidence)
        .bind(entry.created_at)
        .bind(entry.updated_at)
        .execute(pool)
        .await
        .map_err(ApiError::sql)?;

        for tag in &entry.tags {
            sqlx::query(
                "INSERT INTO memory_tags (memory_entry_id, tag) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            )
            .bind(memory_id)
            .bind(tag)
            .execute(pool)
            .await
            .map_err(ApiError::sql)?;
        }

        for source in &entry.sources {
            sqlx::query(
                r#"
                INSERT INTO memory_sources
                    (id, memory_entry_id, task_id, file_path, git_commit, symbol_name, symbol_kind,
                     source_kind, excerpt, created_at)
                VALUES ($1, $2, NULL, $3, $4, $5, $6, $7, $8, now())
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(memory_id)
            .bind(&source.file_path)
            .bind(&source.git_commit)
            .bind(&source.symbol_name)
            .bind(&source.symbol_kind)
            .bind(match source.source_kind {
                SourceKind::TaskPrompt => "task_prompt",
                SourceKind::File => "file",
                SourceKind::GitCommit => "git_commit",
                SourceKind::CommandOutput => "command_output",
                SourceKind::Test => "test",
                SourceKind::Note => "note",
                SourceKind::Memory => "memory",
            })
            .bind(&source.excerpt)
            .execute(pool)
            .await
            .map_err(ApiError::sql)?;
        }

        sqlx::query(
            r#"
            INSERT INTO imported_memory_entries (target_project_id, bundle_id, exported_entry_key, entry_hash, memory_entry_id, latest_import_id, imported_at)
            VALUES ($1, $2, $3, $4, $5, $6, now())
            ON CONFLICT (target_project_id, bundle_id, exported_entry_key) DO UPDATE
            SET entry_hash = EXCLUDED.entry_hash,
                memory_entry_id = EXCLUDED.memory_entry_id,
                latest_import_id = EXCLUDED.latest_import_id,
                imported_at = now()
            "#,
        )
        .bind(target_project_id)
        .bind(&loaded.manifest.bundle_id)
        .bind(&entry.entry_key)
        .bind(&hash)
        .bind(memory_id)
        .bind(import_id)
        .execute(pool)
        .await
        .map_err(ApiError::sql)?;

        current_ids.insert(entry.entry_key.clone(), memory_id);
        imported_ids.push(memory_id);
        imported_count += 1;
    }

    for memory_id in &imported_ids {
        refresh_memory_relations(pool, &slug, *memory_id)
            .await
            .map_err(ApiError::sql)?;
    }

    for entry in &loaded.manifest.entries {
        let Some(src_memory_id) = current_ids.get(&entry.entry_key).copied() else {
            continue;
        };
        sqlx::query("DELETE FROM memory_relations WHERE src_memory_id = $1")
            .bind(src_memory_id)
            .execute(pool)
            .await
            .map_err(ApiError::sql)?;
        for relation in &entry.relations {
            if let Some(dst_memory_id) = current_ids.get(&relation.target_entry_key).copied() {
                sqlx::query(
                    r#"
                    INSERT INTO memory_relations (id, src_memory_id, relation_type, dst_memory_id)
                    VALUES ($1, $2, $3, $4)
                    ON CONFLICT DO NOTHING
                    "#,
                )
                .bind(Uuid::new_v4())
                .bind(src_memory_id)
                .bind(relation.relation_type.to_string())
                .bind(dst_memory_id)
                .execute(pool)
                .await
                .map_err(ApiError::sql)?;
            }
        }
    }

    let embedders = state.embedders.read().await;
    rebuild_chunks_for_automatic_creation(
        pool,
        &slug,
        &embedders,
        state
            .automated_embedding_creation_enabled
            .load(Ordering::Relaxed),
    )
    .await
    .map_err(ApiError::io)?;

    notify_project_changed(
        &state,
        slug.clone(),
        None,
        ActivityKind::BundleImport,
        format!(
            "Imported memory bundle {} into {} memory entry/entries.",
            loaded.manifest.bundle_id, imported_count
        ),
        Some(ActivityDetails::BundleTransfer {
            bundle_id: loaded.manifest.bundle_id.clone(),
            item_count: imported_count,
            source_project: Some(loaded.manifest.source_project.clone()),
        }),
    );
    notify_project_refreshed(&state, slug.clone());

    Ok(Json(ProjectMemoryImportResponse {
        target_project: slug,
        bundle_id: loaded.manifest.bundle_id,
        bundle_hash: loaded.manifest.bundle_hash,
        imported_count,
        replaced_count,
        skipped_count,
        relation_count: loaded
            .manifest
            .entries
            .iter()
            .map(|entry| entry.relations.len())
            .sum(),
    }))
}
