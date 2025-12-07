// Memoire Validation Viewer - Frontend Logic

// Global state
let currentChunkId = null;
let currentFrameId = null;
let chunks = [];
let offset = 0;
const limit = 20;

// DOM elements
const elements = {
    player: document.getElementById('player'),
    chunkList: document.getElementById('chunk-list'),
    loadMore: document.getElementById('load-more'),
    monitorSelect: document.getElementById('monitor-select'),
    dateFilter: document.getElementById('date-filter'),
    frameIdInput: document.getElementById('frame-id'),
    seekFrame: document.getElementById('seek-frame'),
    timestampInput: document.getElementById('timestamp'),
    seekTimestamp: document.getElementById('seek-timestamp'),
    prevFrame: document.getElementById('prev-frame'),
    nextFrame: document.getElementById('next-frame'),
    validationResults: document.getElementById('validation-results'),
    statusIndicator: document.getElementById('status-indicator'),
    statusText: document.getElementById('status-text'),
    ocrText: document.getElementById('ocr-text'),
    ocrProgress: document.getElementById('ocr-progress'),
    ocrStatsText: document.getElementById('ocr-stats-text'),
    searchInput: document.getElementById('search-input'),
    searchBtn: document.getElementById('search-btn'),
    searchResults: document.getElementById('search-results'),
};

// Initialize on page load
document.addEventListener('DOMContentLoaded', init);

async function init() {
    setupEventListeners();
    await loadChunks();
    startOcrStatsPolling();
    updateStatus('Ready', 'green');
}

function setupEventListeners() {
    elements.loadMore.addEventListener('click', loadMoreChunks);
    elements.seekFrame.addEventListener('click', handleSeekToFrame);
    elements.seekTimestamp.addEventListener('click', handleSeekToTimestamp);
    elements.prevFrame.addEventListener('click', () => navigateFrame(-1));
    elements.nextFrame.addEventListener('click', () => navigateFrame(1));

    elements.player.addEventListener('timeupdate', handleTimeUpdate);
    elements.player.addEventListener('seeked', handleSeeked);
    elements.player.addEventListener('loadedmetadata', handleVideoLoaded);

    // Search functionality
    elements.searchBtn.addEventListener('click', handleSearch);
    elements.searchInput.addEventListener('keypress', (e) => {
        if (e.key === 'Enter') {
            handleSearch();
        }
    });
}

// Load chunks from API
async function loadChunks() {
    try {
        updateStatus('Loading chunks...', 'yellow');

        const params = new URLSearchParams({
            limit,
            offset,
        });

        const response = await fetch(`/api/chunks?${params}`);
        const data = await response.json();

        if (data.chunks && data.chunks.length > 0) {
            chunks = chunks.concat(data.chunks);
            renderChunks(data.chunks);
            offset += data.chunks.length;
        } else {
            elements.loadMore.disabled = true;
            elements.loadMore.textContent = 'No more chunks';
        }

        updateStatus('Ready', 'green');
    } catch (error) {
        console.error('Failed to load chunks:', error);
        updateStatus('Error loading chunks', 'red');
    }
}

async function loadMoreChunks() {
    await loadChunks();
}

function renderChunks(newChunks) {
    newChunks.forEach(chunk => {
        const item = document.createElement('div');
        item.className = 'chunk-item';
        item.dataset.chunkId = chunk.id;

        const info = document.createElement('div');
        const date = new Date(chunk.created_at);
        info.innerHTML = `
            <strong>${date.toLocaleTimeString()}</strong> -
            ${chunk.device_name} -
            ${chunk.frame_count} frames
        `;

        const playBtn = document.createElement('button');
        playBtn.className = 'btn btn-small';
        playBtn.textContent = 'Play';
        playBtn.onclick = () => playChunk(chunk.id);

        item.appendChild(info);
        item.appendChild(playBtn);
        elements.chunkList.appendChild(item);
    });
}

async function playChunk(chunkId) {
    try {
        updateStatus('Loading video...', 'yellow');

        currentChunkId = chunkId;
        elements.player.src = `/video/${chunkId}`;

        // Highlight active chunk
        document.querySelectorAll('.chunk-item').forEach(item => {
            item.classList.toggle('active', item.dataset.chunkId == chunkId);
        });

        updateStatus('Playing', 'green');
    } catch (error) {
        console.error('Failed to play chunk:', error);
        updateStatus('Error playing video', 'red');
    }
}

async function handleSeekToFrame() {
    const frameId = parseInt(elements.frameIdInput.value);
    if (!frameId) return;

    await seekToFrame(frameId);
}

async function seekToFrame(frameId) {
    try {
        updateStatus('Seeking to frame...', 'yellow');

        const response = await fetch(`/api/frames/${frameId}`);
        if (!response.ok) {
            throw new Error('Frame not found');
        }

        const frame = await response.json();

        // Load chunk if different
        if (currentChunkId !== frame.video_chunk_id) {
            await playChunk(frame.video_chunk_id);
            // Wait for video to load
            await new Promise(resolve => {
                elements.player.addEventListener('loadedmetadata', resolve, { once: true });
            });
        }

        // Calculate time offset (assuming 1 FPS)
        const fps = 1;
        const targetTime = frame.offset_index / fps;

        elements.player.currentTime = targetTime;
        currentFrameId = frameId;

        displayFrameMetadata(frame);
        updateStatus('Frame loaded', 'green');
    } catch (error) {
        console.error('Failed to seek to frame:', error);
        updateStatus('Error seeking to frame', 'red');
    }
}

async function handleSeekToTimestamp() {
    const timestamp = elements.timestampInput.value;
    if (!timestamp) return;

    // TODO: Implement timestamp seeking
    console.log('Seek to timestamp:', timestamp);
}

async function navigateFrame(direction) {
    if (!currentFrameId) return;

    const nextFrameId = currentFrameId + direction;
    await seekToFrame(nextFrameId);
}

function handleTimeUpdate() {
    // Debounce metadata updates
    if (this.updateTimeout) clearTimeout(this.updateTimeout);

    this.updateTimeout = setTimeout(async () => {
        // TODO: Fetch frame metadata for current playback time
    }, 200);
}

function handleSeeked() {
    // Validate seek accuracy
    if (currentFrameId) {
        validateSeekAccuracy();
    }
}

function handleVideoLoaded() {
    updateValidation('MP4 file playable', 'success');
}

function displayFrameMetadata(frame) {
    document.getElementById('meta-frame-id').textContent = frame.id;
    document.getElementById('meta-timestamp').textContent = frame.timestamp;
    document.getElementById('meta-offset').textContent = frame.offset_index;
    document.getElementById('meta-chunk').textContent = frame.chunk.file_path;
    document.getElementById('meta-monitor').textContent = frame.chunk.device_name;
    document.getElementById('meta-app').textContent = frame.app_name || '-';
    document.getElementById('meta-window').textContent = frame.window_name || '-';
    document.getElementById('meta-url').textContent = frame.browser_url || '-';
    document.getElementById('meta-focused').textContent = frame.focused ? 'Yes' : 'No';

    // Display OCR text
    displayOcrText(frame);
}

async function displayOcrText(frame) {
    const ocrContainer = elements.ocrText;

    if (!frame.ocr_text || frame.ocr_text.length === 0) {
        // Check if OCR is still processing
        try {
            const statsResponse = await fetch('/api/stats/ocr');
            const stats = await statsResponse.json();

            if (stats.processed < stats.total) {
                ocrContainer.innerHTML = '<div class="ocr-processing">⏳ OCR processing in progress... Check back soon.</div>';
            } else {
                ocrContainer.innerHTML = '<div class="ocr-empty">No text detected in this frame</div>';
            }
        } catch (error) {
            ocrContainer.innerHTML = '<div class="ocr-empty">No text detected in this frame</div>';
        }
        return;
    }

    // Display OCR text
    const textContent = frame.ocr_text.map(ocr => ocr.text).join('\n\n');
    ocrContainer.innerHTML = `<div>${escapeHtml(textContent)}</div>`;
}

async function validateSeekAccuracy() {
    if (!currentFrameId) return;

    try {
        const response = await fetch(`/api/frames/${currentFrameId}`);
        const frame = await response.json();

        const expectedTime = frame.offset_index / 1; // 1 FPS
        const actualTime = elements.player.currentTime;
        const drift = Math.abs(actualTime - expectedTime) * 1000; // milliseconds

        if (drift < 50) {
            updateValidation(`Seeking accurate (±${drift.toFixed(0)}ms)`, 'success');
        } else if (drift < 100) {
            updateValidation(`Timestamp drift: +${drift.toFixed(0)}ms (acceptable)`, 'warning');
        } else {
            updateValidation(`Timestamp drift: +${drift.toFixed(0)}ms (high!)`, 'error');
        }
    } catch (error) {
        console.error('Validation failed:', error);
    }
}

function updateValidation(message, type) {
    const list = elements.validationResults;

    // Remove pending message
    const pending = list.querySelector('.pending');
    if (pending) pending.remove();

    // Check if message already exists
    const existing = Array.from(list.children).find(li => li.textContent.includes(message.split(':')[0]));
    if (existing) {
        existing.className = type;
        existing.textContent = getIcon(type) + ' ' + message;
    } else {
        const item = document.createElement('li');
        item.className = type;
        item.textContent = getIcon(type) + ' ' + message;
        list.appendChild(item);
    }
}

function getIcon(type) {
    switch (type) {
        case 'success': return '✓';
        case 'warning': return '⚠';
        case 'error': return '✗';
        default: return '⏳';
    }
}

function updateStatus(text, color) {
    elements.statusText.textContent = text;
    elements.statusIndicator.style.color = `var(--accent-${color})`;
}

// OCR Stats Polling
let ocrStatsInterval = null;

function startOcrStatsPolling() {
    // Poll immediately
    updateOcrStats();

    // Then poll every 5 seconds
    ocrStatsInterval = setInterval(updateOcrStats, 5000);
}

async function updateOcrStats() {
    try {
        const response = await fetch('/api/stats/ocr');
        const stats = await response.json();

        const percentage = stats.total > 0 ? (stats.processed / stats.total) * 100 : 0;

        elements.ocrProgress.style.width = `${percentage}%`;
        elements.ocrStatsText.textContent = `${stats.processed} / ${stats.total} frames processed (${percentage.toFixed(1)}%)`;

        // Stop polling if complete
        if (stats.processed >= stats.total && stats.total > 0) {
            clearInterval(ocrStatsInterval);
            ocrStatsInterval = null;
        }
    } catch (error) {
        console.error('Failed to fetch OCR stats:', error);
        elements.ocrStatsText.textContent = 'Stats unavailable';
    }
}

// Search Functionality
async function handleSearch() {
    const query = elements.searchInput.value.trim();

    if (!query) {
        elements.searchResults.innerHTML = '<div class="search-empty">Enter a search query to find text in OCR data</div>';
        return;
    }

    try {
        elements.searchResults.innerHTML = '<div class="search-loading">⏳ Searching...</div>';

        const params = new URLSearchParams({ query });
        const response = await fetch(`/api/search?${params}`);

        if (!response.ok) {
            throw new Error('Search failed');
        }

        const results = await response.json();

        if (!results || results.length === 0) {
            elements.searchResults.innerHTML = '<div class="search-empty">No results found for your query</div>';
            return;
        }

        displaySearchResults(results);
    } catch (error) {
        console.error('Search error:', error);
        elements.searchResults.innerHTML = '<div class="search-empty">Search failed. Please try again.</div>';
    }
}

function displaySearchResults(results) {
    const query = elements.searchInput.value.trim();

    elements.searchResults.innerHTML = '';

    results.forEach(result => {
        const card = document.createElement('div');
        card.className = 'search-result-card';
        card.onclick = () => handleSearchResultClick(result.frame_id);

        const timestamp = new Date(result.timestamp).toLocaleString();

        // Highlight search terms in snippet
        const highlightedSnippet = highlightText(result.text, query);

        card.innerHTML = `
            <div class="search-result-header">
                <div class="search-result-timestamp">${timestamp}</div>
                <div class="search-result-score">Score: ${result.score.toFixed(2)}</div>
            </div>
            <div class="search-result-snippet">${highlightedSnippet}</div>
            <div class="search-result-meta">
                Frame #${result.frame_id} - ${result.app_name || 'Unknown App'} - ${result.window_name || 'Unknown Window'}
            </div>
        `;

        elements.searchResults.appendChild(card);
    });
}

async function handleSearchResultClick(frameId) {
    await seekToFrame(frameId);

    // Scroll to video player
    document.querySelector('.video-player').scrollIntoView({ behavior: 'smooth' });
}

function highlightText(text, query) {
    const escapedText = escapeHtml(text);
    const escapedQuery = escapeRegex(query);

    // Create case-insensitive regex
    const regex = new RegExp(`(${escapedQuery})`, 'gi');

    return escapedText.replace(regex, '<mark>$1</mark>');
}

function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

function escapeRegex(str) {
    return str.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}
