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
};

// Initialize on page load
document.addEventListener('DOMContentLoaded', init);

async function init() {
    setupEventListeners();
    await loadChunks();
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
